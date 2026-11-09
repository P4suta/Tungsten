use super::{Attachment, Note, Store, catalog::Catalog, file_error, open_file};
use crate::{
    Error,
    domain::{NoteName, Revision},
};
use rusqlite::{OptionalExtension, params};
use rustix::fs::{self, Mode, OFlags};
use std::{
    fs::{File, TryLockError},
    io,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) fn now() -> Result<i64, Error> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_millis(),
    )
    .map_err(|_| Error::Worker)
}

pub(crate) struct EncryptedDirectory {
    pub(super) path: PathBuf,
    pub(super) root: File,
    pub(super) lock: File,
}

impl EncryptedDirectory {
    #[cfg(feature = "server")]
    pub(crate) fn duplicate(&self) -> Result<Self, Error> {
        Ok(Self {
            path: self.path.clone(),
            root: self.root.try_clone()?,
            lock: self.lock.try_clone()?,
        })
    }
    pub(crate) fn manifest(&self) -> Result<Option<crate::sealed::RuntimeManifest>, Error> {
        match super::resources::read_bytes(&self.root, ".vault.json", 2048) {
            Ok(bytes) => {
                let manifest: crate::sealed::RuntimeManifest =
                    serde_json::from_slice(&bytes).map_err(|_| Error::InvalidFile)?;
                manifest.validate()?;
                if manifest.phase == crate::sealed::VaultPhase::Ready {
                    let database = open_file(&self.root, ".sealed.sqlite", OFlags::RDONLY | OFlags::NONBLOCK).map_err(|_| Error::InvalidData("The encrypted database is missing. Restore the complete encrypted data directory."))?;
                    if database.metadata()?.len() < 4096 {
                        return Err(Error::InvalidData("The encrypted database is incomplete."));
                    }
                }
                Ok(Some(manifest))
            }
            Err(Error::NotFound) => {
                if [
                    ".sealed.sqlite",
                    ".sealed.sqlite-wal",
                    ".sealed.sqlite-shm",
                    ".sealed.sqlite-journal",
                ]
                .iter()
                .any(|name| self.path.join(name).symlink_metadata().is_ok())
                {
                    return Err(Error::InvalidData(
                        "The vault manifest is missing. Restore the complete encrypted data directory.",
                    ));
                }
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    #[cfg(feature = "server")]
    pub(crate) fn save_manifest(
        &self,
        manifest: &crate::sealed::RuntimeManifest,
    ) -> Result<(), Error> {
        manifest.validate()?;
        let current = self.manifest()?;
        let replace = match tungsten_state::manifest_change(
            current.as_ref().map(|current| current.phase),
            manifest.phase,
            current
                .as_ref()
                .is_some_and(|current| current.vault == manifest.vault),
            current.as_ref() == Some(manifest),
        ) {
            tungsten_state::ManifestChange::Create => false,
            tungsten_state::ManifestChange::Replace => true,
            tungsten_state::ManifestChange::Unchanged => return Ok(()),
            tungsten_state::ManifestChange::Reject => return Err(Error::Exists),
        };
        let bytes = serde_json::to_vec(manifest).map_err(|_| Error::InvalidFile)?;
        super::atomic_write_with_prefix(
            &self.root,
            ".vault.json",
            replace,
            ".tungsten-manifest-",
            |file| std::io::Write::write_all(file, &bytes),
        )?;
        Ok(())
    }

    pub(crate) fn open(path: &Path) -> Result<Self, Error> {
        std::fs::create_dir_all(path)?;
        let root = File::from(
            fs::open(
                path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(io::Error::from)?,
        );
        let mut staging = Vec::new();
        for (index, entry) in std::fs::read_dir(path)?.enumerate() {
            if index >= 64 {
                return Err(Error::InvalidData(
                    "The encrypted data directory contains too many unexpected entries.",
                ));
            }
            let entry = entry?;
            let temporary = entry.file_name().to_str().is_some_and(|name| {
                name.strip_prefix(".tungsten-manifest-")
                    .and_then(|name| name.strip_suffix(".tmp"))
                    .is_some_and(|id| crate::rules::valid_id(id.as_bytes()))
            });
            if !entry.file_type()?.is_file()
                || entry.metadata()?.nlink() != 1
                || (!temporary
                    && !matches!(
                        entry.file_name().to_str(),
                        Some(
                            ".sealed.sqlite"
                                | ".sealed.sqlite-wal"
                                | ".sealed.sqlite-shm"
                                | ".sealed.sqlite-journal"
                                | ".vault.json"
                                | ".tungsten.lock"
                        )
                    ))
            {
                return Err(Error::InvalidData(
                    "Use an empty or encrypted data directory.",
                ));
            }
            if temporary {
                if entry.metadata()?.len() > 2048 {
                    return Err(Error::InvalidFile);
                }
                staging.push(entry.file_name());
            }
        }
        root.set_permissions(std::fs::Permissions::from_mode(0o700))?;
        let lock = open_file(&root, ".tungsten.lock", OFlags::RDWR | OFlags::CREATE)?;
        lock.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => Error::Locked,
            TryLockError::Error(error) => Error::Io(error),
        })?;
        for name in staging {
            fs::unlinkat(&root, &name, fs::AtFlags::empty()).map_err(io::Error::from)?;
        }
        root.sync_all()?;
        Ok(Self {
            path: path.canonicalize()?,
            root,
            lock,
        })
    }

    pub(crate) fn store(&self, key: &[u8; 32]) -> Result<Store, Error> {
        let initialize = if self.path.join(".vault.json").try_exists()? {
            self.manifest()?
                .is_some_and(|manifest| manifest.phase != crate::sealed::VaultPhase::Ready)
        } else {
            true
        };
        Store::open_encrypted_directory(self, key, initialize)
    }
}

impl Store {
    pub fn import_encrypted(
        &mut self,
        notes: impl Iterator<Item = Result<Note, Error>>,
    ) -> Result<usize, Error> {
        let mut input = notes.peekable();
        let existing = self.index.count()?;
        let building = self
            .index
            .db
            .query_row("SELECT value FROM state WHERE key='building'", [], |row| {
                row.get::<_, bool>(0)
            })
            .optional()?
            .unwrap_or(false);
        if !self.encrypted || (existing != 0 && !building && input.peek().is_some()) {
            return Err(Error::InvalidData(
                "Import requires an empty encrypted store.",
            ));
        }
        if !building && input.peek().is_none() {
            return Ok(existing);
        }
        self.index.begin_import()?;
        self.index
            .db
            .execute("INSERT OR REPLACE INTO state VALUES('building',1)", [])?;
        let mut count = existing;
        while input.peek().is_some() {
            let transaction = self.index.db.unchecked_transaction()?;
            let mut bytes = 0;
            for _ in 0..500 {
                let Some(note) = input.next() else { break };
                let note = note?;
                if note.body.len() > super::MAX_NOTE_BYTES
                    || Revision::of(note.body.as_bytes()) != note.revision
                {
                    return Err(Error::InvalidFile);
                }
                let modified = i64::try_from(note.modified).map_err(|_| Error::InvalidFile)?;
                transaction.execute(
                    "INSERT INTO vault_notes VALUES(?,?,?)",
                    params![note.name.as_str(), note.body, modified],
                )?;
                self.index.put(&note, note.revision.as_str(), false, true)?;
                count += 1;
                bytes += note.body.len();
                if bytes >= 4 * 1024 * 1024 {
                    break;
                }
            }
            transaction.commit()?;
        }
        self.index.finish_rebuild()?;
        Ok(count)
    }

    pub fn reclaim_deleted_pages(&mut self) -> Result<usize, Error> {
        if !self.encrypted {
            return Ok(0);
        }
        let before: u32 = self
            .index
            .db
            .pragma_query_value(None, "freelist_count", |row| row.get(0))?;
        if before == 0 {
            return Ok(0);
        }
        self.index
            .db
            .execute_batch("PRAGMA incremental_vacuum(32)")?;
        let after: u32 = self
            .index
            .db
            .pragma_query_value(None, "freelist_count", |row| row.get(0))?;
        Ok(before.saturating_sub(after).min(32) as usize)
    }

    pub(super) fn encrypted_record(&self, id: &str, trash: bool) -> Result<Note, Error> {
        let (name, body, revision, modified) = self
            .index
            .db
            .query_row(
                "SELECT name,body,revision,modified FROM vault_records WHERE trash=? AND id=?",
                params![trash, id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        let revision = Revision::parse(revision)?;
        if !revision.matches(Revision::of(body.as_bytes()).as_str()) {
            return Err(Error::InvalidFile);
        }
        Ok(Note {
            name: NoteName::parse(name)?,
            body,
            revision,
            modified: u128::try_from(modified).map_err(|_| Error::InvalidFile)?,
        })
    }

    pub(super) fn upload_encrypted_attachment(
        &self,
        id: &str,
        name: &str,
        bytes: &[u8],
    ) -> Result<Attachment, Error> {
        let meta = serde_json::to_vec(name).map_err(|_| Error::InvalidFile)?;
        let revision = Revision::of_parts(&[bytes, &meta]);
        let transaction = self.index.db.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO vault_files VALUES(?,?,?,?,?,?,?)",
            params![
                id,
                &meta,
                &bytes[..bytes.len().min(512)],
                revision.as_str(),
                Revision::of(bytes).as_str(),
                now()?,
                bytes.len() as i64
            ],
        )?;
        transaction.execute(
            "INSERT INTO vault_blobs(id,data) VALUES(?,?)",
            params![id, bytes],
        )?;
        self.register_file(id, SystemTime::now())?;
        transaction.commit()?;
        self.encrypted_attachment(id)
    }

    pub(super) fn encrypted_attachment(&self, id: &str) -> Result<Attachment, Error> {
        if !crate::rules::valid_attachment(id.as_bytes()) {
            return Err(Error::InvalidData("The attachment identifier is invalid."));
        }
        let (meta, head, bytes, revision, data_revision) = self
            .index
            .db
            .query_row(
                "SELECT meta,head,bytes,revision,body_revision FROM vault_files WHERE id=?",
                [id],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        let (kind, mime) = crate::media::content_type(id, &head);
        let meta: super::resources::AttachmentMeta =
            serde_json::from_slice(&meta).map_err(|_| Error::InvalidFile)?;
        meta.validate()?;
        let (name, folder, description) = match meta {
            super::resources::AttachmentMeta::Name(name) => (name, String::new(), String::new()),
            super::resources::AttachmentMeta::Details {
                name,
                folder,
                description,
            } => (name, folder, description),
        };
        Ok(Attachment {
            id: id.to_owned(),
            name,
            bytes: usize::try_from(bytes).map_err(|_| Error::InvalidFile)?,
            revision: Revision::parse(revision)?,
            data_revision: Revision::parse(data_revision)?,
            url: format!("/.attachments/{id}"),
            image: crate::media::image_type(&head).is_some(),
            kind,
            mime,
            folder,
            description,
        })
    }

    pub fn open_encrypted(path: &Path, key: &[u8; 32]) -> Result<Self, Error> {
        EncryptedDirectory::open(path)?.store(key)
    }

    pub(super) fn open_encrypted_directory(
        directory: &EncryptedDirectory,
        key: &[u8; 32],
        initialize: bool,
    ) -> Result<Self, Error> {
        let root = directory.root.try_clone()?;
        let lock = directory.lock.try_clone()?;
        let index =
            Catalog::open_encrypted(&directory.path.join(".sealed.sqlite"), key, initialize)?;
        if initialize {
            index.db.execute_batch("BEGIN IMMEDIATE;
          CREATE TABLE IF NOT EXISTS vault_notes(name TEXT PRIMARY KEY,body TEXT NOT NULL,modified INTEGER NOT NULL) WITHOUT ROWID;
          CREATE TABLE IF NOT EXISTS vault_pins(name TEXT PRIMARY KEY) WITHOUT ROWID;
          CREATE TABLE IF NOT EXISTS vault_records(trash INTEGER NOT NULL,id TEXT NOT NULL,name TEXT NOT NULL,body TEXT NOT NULL,revision TEXT NOT NULL,modified INTEGER NOT NULL,captured INTEGER NOT NULL,PRIMARY KEY(trash,id)) WITHOUT ROWID;
          CREATE TABLE IF NOT EXISTS vault_files(id TEXT PRIMARY KEY,meta BLOB NOT NULL,head BLOB NOT NULL,revision TEXT NOT NULL,body_revision TEXT NOT NULL,modified INTEGER NOT NULL,bytes INTEGER NOT NULL) WITHOUT ROWID;
          CREATE TABLE IF NOT EXISTS vault_blobs(id TEXT NOT NULL UNIQUE REFERENCES vault_files(id) ON DELETE CASCADE,data BLOB NOT NULL);
          INSERT OR REPLACE INTO state VALUES('notes-readable',1),('recovery-readable',1),('file-index',1);
          COMMIT;")?;
        }
        open_file(&root, ".sealed.sqlite", OFlags::RDONLY)
            .map_err(file_error)?
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
        root.sync_all()?;
        Ok(Self {
            encrypted: true,
            trash: root.try_clone()?,
            history: root.try_clone()?,
            attachments: root.try_clone()?,
            pins: root.try_clone()?,
            root,
            index,
            _lock: lock,
        })
    }

    pub(super) fn encrypted_note(&self, name: &NoteName) -> Result<Note, Error> {
        self.index
            .db
            .query_row(
                "SELECT body,modified FROM vault_notes WHERE name=?",
                [name.as_str()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .map(|(body, modified)| {
                Ok(Note {
                    name: name.clone(),
                    revision: Revision::of(body.as_bytes()),
                    body,
                    modified: u128::try_from(modified).map_err(|_| Error::InvalidFile)?,
                })
            })
            .ok_or(Error::NotFound)?
    }

    pub(super) fn write_encrypted_note(
        &self,
        name: &NoteName,
        body: &str,
        replace: bool,
    ) -> Result<(), Error> {
        let transaction = self.index.db.unchecked_transaction()?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM vault_notes WHERE name=?)",
            [name.as_str()],
            |row| row.get(0),
        )?;
        if exists && !replace {
            return Err(Error::Exists);
        }
        transaction.execute("INSERT INTO vault_notes VALUES(?,?,?) ON CONFLICT(name) DO UPDATE SET body=excluded.body,modified=excluded.modified", params![name.as_str(), body, now()?])?;
        self.refresh_index(name, false)?;
        transaction.commit()?;
        Ok(())
    }

    pub(super) fn rename_encrypted_note(
        &self,
        name: &NoteName,
        renamed: &NoteName,
    ) -> Result<Note, Error> {
        if self.index.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM vault_notes WHERE name=?)",
            [renamed.as_str()],
            |row| row.get::<_, bool>(0),
        )? {
            return Err(Error::Exists);
        }
        let transaction = self.index.db.unchecked_transaction()?;
        transaction.execute(
            "UPDATE vault_notes SET name=? WHERE name=?",
            params![renamed.as_str(), name.as_str()],
        )?;
        self.move_metadata(name, renamed)?;
        self.index.remove(name)?;
        self.refresh_index(renamed, false)?;
        transaction.commit()?;
        self.read(renamed)
    }
}
