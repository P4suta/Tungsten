use crate::{
    Error,
    domain::{NoteName, Revision},
};
use rustix::fs::{self, AtFlags, Mode, OFlags, RenameFlags};
use serde::Serialize;
use std::{
    fs::{File, TryLockError},
    io::{self, Read, Write},
    path::Path,
    time::UNIX_EPOCH,
};
use uuid::Uuid;

mod catalog;
mod encrypted;
mod file_collection;
#[cfg(any(feature = "server", test))]
mod migration;
mod recovery_catalog;
mod resources;
pub use catalog::{Graph, Page};
#[cfg(feature = "server")]
pub(crate) use encrypted::EncryptedDirectory;
#[cfg(feature = "server")]
pub(crate) use migration::MigrationStop;
pub use recovery_catalog::TrashPage;
pub use resources::{Attachment, SavedNote, image_type};

pub const MAX_NOTE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct Note {
    pub name: NoteName,
    pub body: String,
    pub modified: u128,
    pub revision: Revision,
}

#[derive(Debug, Serialize, serde::Deserialize)]
pub struct Summary {
    pub name: NoteName,
    pub modified: u128,
    pub pinned: bool,
    pub tags: Vec<String>,
    pub snippet: String,
    pub title: String,
    pub bytes: usize,
}

pub struct Store {
    encrypted: bool,
    root: File,
    trash: File,
    history: File,
    attachments: File,
    pins: File,
    index: catalog::Catalog,
    _lock: File,
}

fn open_file(directory: &File, name: &str, flags: OFlags) -> io::Result<File> {
    fs::openat(
        directory,
        name,
        flags | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::RUSR | Mode::WUSR,
    )
    .map(File::from)
    .map_err(io::Error::from)
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, Error> {
        std::fs::create_dir_all(path)?;
        if path.join(".sealed.sqlite").symlink_metadata().is_ok()
            || path.join(".vault.json").symlink_metadata().is_ok()
        {
            return Err(Error::InvalidData(
                "Unlock the encrypted vault instead of opening plaintext storage.",
            ));
        }
        let root = File::from(
            fs::open(
                path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(io::Error::from)?,
        );
        let lock = open_file(&root, ".tungsten.lock", OFlags::RDWR | OFlags::CREATE)?;
        lock.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => Error::Locked,
            TryLockError::Error(error) => Error::Io(error),
        })?;
        let trash = subdirectory(&root, ".trash")?;
        let history = subdirectory(&root, ".history")?;
        let attachments = subdirectory(&root, ".attachments")?;
        let pins = subdirectory(&root, ".pins")?;
        let _index_directory = subdirectory(&root, ".index")?;
        let index = catalog::Catalog::open(&path.join(".index/catalog.sqlite"))?;
        root.sync_all()?;
        let store = Self {
            encrypted: false,
            root,
            trash,
            history,
            attachments,
            pins,
            index,
            _lock: lock,
        };
        store.migrate_trash()?;
        store.reconcile(false)?;
        store.reconcile_recovery(false)?;
        store.reconcile_files(std::time::SystemTime::now())?;
        Ok(store)
    }

    fn parent(&self, name: &NoteName, create: bool) -> Result<(File, String), Error> {
        let mut directory = self.root.try_clone()?;
        let mut parts = name.as_str().split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                return Ok((directory, part.to_owned()));
            }
            directory = if create {
                subdirectory(&directory, part)?
            } else {
                open_file(&directory, part, OFlags::RDONLY | OFlags::DIRECTORY)
                    .map_err(file_error)?
            };
        }
        Err(Error::InvalidName)
    }

    pub fn read(&self, name: &NoteName) -> Result<Note, Error> {
        if self.encrypted {
            return self.encrypted_note(name);
        }
        let (directory, basename) = self.parent(name, false)?;
        let file = open_file(&directory, &basename, OFlags::RDONLY | OFlags::NONBLOCK)
            .map_err(file_error)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(Error::InvalidFile);
        }
        if metadata.len() > MAX_NOTE_BYTES as u64 {
            return Err(Error::TooLarge);
        }
        let mut bytes = Vec::new();
        file.take((MAX_NOTE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_NOTE_BYTES {
            return Err(Error::TooLarge);
        }
        let revision = Revision::of(&bytes);
        let body = String::from_utf8(bytes).map_err(|_| Error::InvalidFile)?;
        let modified = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_millis();
        Ok(Note {
            name: name.clone(),
            body,
            modified,
            revision,
        })
    }

    pub fn read_durable(&self, name: &NoteName) -> Result<Note, Error> {
        let note = self.read(name)?;
        if self.encrypted {
            return Ok(note);
        }
        let (directory, basename) = self.parent(name, false)?;
        open_file(&directory, &basename, OFlags::RDONLY | OFlags::NONBLOCK)?.sync_all()?;
        directory.sync_all()?;
        self.root.sync_all()?;
        Ok(note)
    }

    pub fn list(&self, query: &str) -> Result<Vec<Summary>, Error> {
        Ok(self.search(query, None, 200, false)?.notes)
    }

    pub fn search(
        &self,
        query: &str,
        cursor: Option<&str>,
        limit: usize,
        titles: bool,
    ) -> Result<Page, Error> {
        self.repair_pending()?;
        self.index.search(query, cursor, limit, titles)
    }

    pub fn graph(&self, focus: Option<&NoteName>, limit: usize) -> Result<Graph, Error> {
        self.repair_pending()?;
        self.index.graph(focus, limit)
    }

    pub fn backlinks(&self, name: &NoteName) -> Result<Vec<Summary>, Error> {
        self.repair_pending()?;
        self.index
            .incoming(name, 0, 120)?
            .into_iter()
            .map(|(_, name)| self.index.summary(name.as_str())?.ok_or(Error::NotFound))
            .collect()
    }

    pub fn attachment_sources(&self, id: &str) -> Result<Vec<Summary>, Error> {
        self.repair_pending()?;
        self.index.sources_for_attachment(id, 120)
    }

    pub fn connection(&self, name: &NoteName) -> Result<(Vec<String>, Vec<String>), Error> {
        let mut links = self.index.outgoing(name)?;
        links.truncate(120);
        let mut files = self.index.attachment_links(name)?;
        files.truncate(120);
        Ok((links, files))
    }

    pub fn process_link_updates(&mut self, limit: usize) -> Result<usize, Error> {
        use rusqlite::OptionalExtension;
        if !(1..=256).contains(&limit) {
            return Err(Error::InvalidData("The link update batch is invalid."));
        }
        let job=self.index.db.query_row("SELECT id,target,previous,title,after_id FROM retitles WHERE ready=1 ORDER BY id LIMIT 1",[],|row|Ok((row.get::<_,i64>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?,row.get::<_,i64>(4)?))).optional()?;
        let Some((id, target, previous, title, after)) = job else {
            return Ok(0);
        };
        let target = NoteName::parse(target)?;
        if matches!(self.read(&target), Err(Error::NotFound)) {
            self.index
                .db
                .execute("DELETE FROM retitles WHERE id=?", [id])?;
            return Ok(0);
        }
        let sources = self
            .index
            .retitle_candidates(&target, &previous, after, limit)?;
        let started = std::time::Instant::now();
        let mut processed = 0;
        let mut written = 0;
        let mut last = after;
        for (source_id, name) in &sources {
            if processed > 0
                && (written >= 16 || started.elapsed() >= std::time::Duration::from_millis(25))
            {
                break;
            }
            let note = self.read(name)?;
            let updated = crate::markdown::retitle_links(
                name.as_str(),
                &note.body,
                target.as_str(),
                &previous,
                &title,
            );
            if updated != note.body {
                if updated.len() > MAX_NOTE_BYTES {
                    eprintln!("operation=link_update outcome=source_limit");
                    last = *source_id;
                    processed += 1;
                    continue;
                }
                self.snapshot(&note)?;
                self.write(name, &updated, true)?;
                written += 1;
            }
            last = *source_id;
            processed += 1;
        }
        self.index.db.execute(
            "UPDATE retitles SET after_id=? WHERE id=?",
            rusqlite::params![last, id],
        )?;
        if sources.len() < limit && processed == sources.len() {
            self.index
                .db
                .execute("DELETE FROM retitles WHERE id=?", [id])?;
        }
        Ok(processed)
    }

    fn stamp(&self, name: &NoteName) -> Result<String, Error> {
        if self.encrypted {
            return Ok(self.encrypted_note(name)?.revision.to_string());
        }
        use std::os::unix::fs::MetadataExt;
        let (directory, basename) = self.parent(name, false)?;
        let file = open_file(&directory, &basename, OFlags::RDONLY | OFlags::NONBLOCK)
            .map_err(file_error)?;
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(Error::InvalidFile);
        }
        Ok(format!(
            "{}:{}:{}:{}:{}:{}",
            meta.ino(),
            meta.len(),
            meta.mtime(),
            meta.mtime_nsec(),
            meta.ctime(),
            meta.ctime_nsec()
        ))
    }

    fn refresh_index(&self, name: &NoteName, bulk: bool) -> Result<(), Error> {
        match self.read(name) {
            Ok(note) => self
                .index
                .put(&note, &self.stamp(name)?, self.pinned(name)?, bulk),
            Err(Error::NotFound) => self.index.remove(name),
            Err(error) => Err(error),
        }
    }

    fn repair_pending(&self) -> Result<(), Error> {
        let pending = self.index.pending()?;
        if !pending.is_empty() {
            let transaction = self.index.db.unchecked_transaction()?;
            for name in pending {
                self.refresh_index(&name, false)?;
            }
            transaction.commit()?;
        }
        Ok(())
    }

    fn reconcile(&self, strict: bool) -> Result<(), Error> {
        if self.encrypted {
            return self.repair_pending();
        }
        self.index.db.execute(
            "INSERT OR REPLACE INTO state VALUES('notes-readable',0)",
            [],
        )?;
        let mut readable = true;
        let mut names = Vec::new();
        collect_names(&self.root, "", 0, &mut names)?;
        let existing: std::collections::BTreeSet<_> =
            names.iter().map(|name| name.as_str()).collect();
        use rusqlite::OptionalExtension;
        let interrupted = self
            .index
            .db
            .query_row("SELECT value FROM state WHERE key='building'", [], |row| {
                row.get::<_, bool>(0)
            })
            .optional()?
            .unwrap_or(false);
        if interrupted || self.index.count()? == 0 {
            self.index.begin_import()?;
        }
        self.index
            .db
            .execute("INSERT OR REPLACE INTO state VALUES('building',1)", [])?;
        let bulk = true;
        let mut changed = 0;
        self.index.db.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> Result<(), Error> {
            for name in &names {
                let stamp = match self.stamp(name) {
                    Ok(stamp) => stamp,
                    Err(Error::InvalidFile | Error::TooLarge | Error::NotFound) if !strict => {
                        readable = false;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                if self.index.stamp(name)?.as_deref() != Some(&stamp) {
                    match self.refresh_index(name, bulk) {
                        Ok(()) => {}
                        Err(Error::InvalidFile | Error::TooLarge | Error::NotFound) if !strict => {
                            readable = false;
                            self.index.remove(name)?;
                        }
                        Err(error) => return Err(error),
                    }
                    changed += 1;
                    if changed % 1000 == 0 {
                        self.index.db.execute_batch("COMMIT; BEGIN IMMEDIATE")?;
                    }
                    if changed % 10_000 == 0 {
                        eprintln!(
                            "operation=index phase=reconcile changed={changed} total={}",
                            names.len()
                        );
                    }
                }
            }
            for name in self.index.names()? {
                if !existing.contains(name.as_str()) {
                    self.index.remove(&name)?;
                    changed += 1;
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            let _ = self.index.db.execute_batch("ROLLBACK");
            return Err(error);
        }
        self.index.db.execute_batch("COMMIT")?;
        self.index.db.execute(
            "UPDATE state SET value=? WHERE key='notes-readable'",
            [readable],
        )?;
        self.repair_pending()?;
        if changed > 0 || interrupted {
            self.index.finish_rebuild()?;
        } else {
            self.index
                .db
                .execute("UPDATE state SET value=0 WHERE key='building'", [])?;
            self.index.ensure_indexes()?;
        }
        eprintln!(
            "operation=index phase=ready notes={} changed={changed}",
            self.index.count()?
        );
        Ok(())
    }

    pub fn create(&mut self, name: Option<String>, body: &str) -> Result<Note, Error> {
        let name = NoteName::parse(match name {
            Some(name) if !name.is_empty() => name,
            _ => format!(
                "Note-{}-{}.md",
                std::time::SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(io::Error::other)?
                    .as_millis(),
                &Uuid::new_v4().simple().to_string()[..8]
            ),
        })?;
        self.write(&name, body, false)?;
        self.read(&name)
    }

    fn checked(&self, name: &NoteName, expected: &str) -> Result<Note, Error> {
        let current = self.read(name)?;
        if !current.revision.matches(expected) {
            return Err(Error::Conflict);
        }
        Ok(current)
    }

    fn write(&self, name: &NoteName, body: &str, replace: bool) -> Result<(), Error> {
        if body.len() > MAX_NOTE_BYTES {
            return Err(Error::TooLarge);
        }
        if self.encrypted {
            return self.write_encrypted_note(name, body, replace);
        }
        let (directory, basename) = self.parent(name, true)?;
        self.index.journal(&[name])?;
        atomic_write(&directory, &basename, replace, |file| {
            file.write_all(body.as_bytes())
        })
        .map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                Error::Exists
            } else {
                Error::Io(error)
            }
        })?;
        let transaction = self.index.db.unchecked_transaction()?;
        self.refresh_index(name, false)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn update(&mut self, name: &NoteName, body: &str, expected: &str) -> Result<Note, Error> {
        if body.len() > MAX_NOTE_BYTES {
            return Err(Error::TooLarge);
        }
        let current = self.checked(name, expected)?;
        if current.body == body {
            return Ok(current);
        }
        let previous = crate::markdown::first_heading(&current.body);
        let title = crate::markdown::first_heading(body);
        let mut linked = Vec::new();
        if let (Some(previous), Some(title)) = (previous, title)
            && previous != title
        {
            let sources = self.index.incoming(name, 0, 17)?;
            if sources.len() > 16 {
                self.index.queue_title(
                    name,
                    &previous,
                    &title,
                    Revision::of(body.as_bytes()).as_str(),
                )?;
            } else {
                for (_, source) in sources {
                    if &source == name {
                        continue;
                    }
                    let note = self.read(&source)?;
                    let updated = crate::markdown::retitle_links(
                        source.as_str(),
                        &note.body,
                        name.as_str(),
                        &previous,
                        &title,
                    );
                    if updated.len() > MAX_NOTE_BYTES {
                        return Err(Error::TooLarge);
                    }
                    if updated != note.body {
                        linked.push((note, updated));
                    }
                }
            }
        }
        self.snapshot(&current)?;
        for (note, updated) in linked {
            self.snapshot(&note)?;
            self.write(&note.name, &updated, true)?;
        }
        self.write(name, body, true)?;
        self.read(name)
    }

    pub fn rename(
        &mut self,
        name: &NoteName,
        renamed: &NoteName,
        expected: &str,
    ) -> Result<Note, Error> {
        let current = self.checked(name, expected)?;
        if name == renamed {
            return Ok(current);
        }
        if self.encrypted {
            return self.rename_encrypted_note(name, renamed);
        }
        let (source, source_name) = self.parent(name, false)?;
        let (destination, destination_name) = self.parent(renamed, true)?;
        self.index.journal(&[name, renamed])?;
        fs::renameat_with(
            &source,
            source_name.as_str(),
            &destination,
            destination_name.as_str(),
            RenameFlags::NOREPLACE,
        )
        .map_err(|error| {
            if error == rustix::io::Errno::EXIST {
                Error::Exists
            } else {
                Error::Io(error.into())
            }
        })?;
        source.sync_all()?;
        destination.sync_all()?;
        self.root.sync_all()?;
        self.move_metadata(name, renamed)?;
        let transaction = self.index.db.unchecked_transaction()?;
        self.index.remove(name)?;
        self.refresh_index(renamed, false)?;
        transaction.commit()?;
        self.read(renamed)
    }

    pub fn delete(&mut self, name: &NoteName, expected: &str) -> Result<(), Error> {
        let note = self.checked(name, expected)?;
        self.save_record(&self.trash, &note)?;
        if self.encrypted {
            let transaction = self.index.db.unchecked_transaction()?;
            transaction.execute("DELETE FROM vault_notes WHERE name=?", [name.as_str()])?;
            self.set_pin(name, false)?;
            self.index.remove(name)?;
            transaction.commit()?;
            return Ok(());
        }
        self.index.journal(&[name])?;
        let (directory, basename) = self.parent(name, false)?;
        fs::unlinkat(&directory, basename.as_str(), AtFlags::empty()).map_err(io::Error::from)?;
        directory.sync_all()?;
        self.trash.sync_all()?;
        self.root.sync_all()?;
        self.set_pin(name, false)?;
        let transaction = self.index.db.unchecked_transaction()?;
        self.index.remove(name)?;
        transaction.commit()?;
        Ok(())
    }
}

fn file_error(error: io::Error) -> Error {
    match error.kind() {
        io::ErrorKind::NotFound => Error::NotFound,
        _ if error.raw_os_error() == Some(rustix::io::Errno::LOOP.raw_os_error()) => {
            Error::InvalidFile
        }
        _ => Error::Io(error),
    }
}

fn subdirectory(parent: &File, name: &str) -> Result<File, Error> {
    match fs::mkdirat(parent, name, Mode::RWXU) {
        Ok(()) => parent.sync_all()?,
        Err(rustix::io::Errno::EXIST) => {}
        Err(error) => return Err(Error::Io(error.into())),
    }
    open_file(parent, name, OFlags::RDONLY | OFlags::DIRECTORY).map_err(file_error)
}

fn collect_names(
    directory: &File,
    prefix: &str,
    depth: usize,
    names: &mut Vec<NoteName>,
) -> Result<(), Error> {
    if depth >= 16 {
        return Ok(());
    }
    for entry in fs::Dir::read_from(directory).map_err(io::Error::from)? {
        let entry = entry.map_err(io::Error::from)?;
        let Ok(text) = entry.file_name().to_str() else {
            continue;
        };
        if text.starts_with('.') {
            continue;
        }
        let path = format!("{prefix}{text}");
        if entry.file_type() == fs::FileType::Directory {
            let child = open_file(directory, text, OFlags::RDONLY | OFlags::DIRECTORY)?;
            collect_names(&child, &format!("{path}/"), depth + 1, names)?;
        } else if let Ok(name) = NoteName::parse(path) {
            names.push(name);
        }
    }
    Ok(())
}

struct Temporary<'a> {
    directory: &'a File,
    name: String,
    active: bool,
}

impl Drop for Temporary<'_> {
    fn drop(&mut self) {
        if self.active
            && let Err(error) = fs::unlinkat(self.directory, self.name.as_str(), AtFlags::empty())
        {
            eprintln!("operation=temporary_cleanup outcome=failed error={error}");
        }
    }
}

fn atomic_write(
    directory: &File,
    name: &str,
    replace: bool,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    atomic_write_with_prefix(directory, name, replace, ".tungsten-", write)
}

fn atomic_write_with_prefix(
    directory: &File,
    name: &str,
    replace: bool,
    prefix: &str,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    let temporary_name = format!("{prefix}{}.tmp", Uuid::new_v4().simple());
    let mut file = open_file(
        directory,
        &temporary_name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL,
    )?;
    let mut temporary = Temporary {
        directory,
        name: temporary_name,
        active: true,
    };
    write(&mut file)?;
    file.sync_all()?;
    if replace {
        fs::renameat(directory, temporary.name.as_str(), directory, name)
            .map_err(io::Error::from)?;
    } else {
        fs::renameat_with(
            directory,
            temporary.name.as_str(),
            directory,
            name,
            RenameFlags::NOREPLACE,
        )
        .map_err(io::Error::from)?;
    }
    temporary.active = false;
    directory.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_write_keeps_the_saved_file_and_cleans_the_temporary() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        std::fs::write(directory.path().join("Note.md"), "saved").unwrap();
        let result = atomic_write(&store.root, "Note.md", true, |file| {
            file.write_all(b"partial")?;
            Err(io::Error::other("injected write failure"))
        });
        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(directory.path().join("Note.md")).unwrap(),
            "saved"
        );
        assert!(std::fs::read_dir(directory.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        }));
    }
}
