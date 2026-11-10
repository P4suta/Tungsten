use super::{Attachment, Note, Store, now};
use crate::{
    Error,
    domain::{NoteName, Revision},
};
use rusqlite::{OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::time::SystemTime;

impl Store {
    pub fn import_notes(
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
        if existing != 0 && !building && input.peek().is_some() {
            return Err(Error::InvalidData("Import requires an empty database."));
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
                    "INSERT INTO documents VALUES(?,?,?)",
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

    pub(super) fn record_body(&self, id: &str, trash: bool) -> Result<Note, Error> {
        let (name, body, revision, modified) = self
            .index
            .db
            .query_row(
                "SELECT name,body,revision,modified FROM snapshots WHERE trash=? AND id=?",
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

    pub(super) fn insert_attachment(
        &self,
        id: &str,
        name: &str,
        length: usize,
        head: &[u8],
        mut source: impl Read,
    ) -> Result<Attachment, Error> {
        let meta = serde_json::to_vec(name).map_err(|_| Error::InvalidFile)?;
        let pending = Revision::of(b"");
        let transaction = self.index.db.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO files VALUES(?,?,?,?,?,?,?)",
            params![
                id,
                &meta,
                head,
                pending.as_str(),
                pending.as_str(),
                now()?,
                length as i64
            ],
        )?;
        transaction.execute(
            "INSERT INTO file_data(id,data) VALUES(?,?)",
            params![id, rusqlite::blob::ZeroBlob(length as u64)],
        )?;
        let mut blob = self.index.db.blob_open(
            "main",
            "file_data",
            "data",
            transaction.last_insert_rowid(),
            false,
        )?;
        let mut digest = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        let mut offset = 0;
        while offset < length {
            let bytes = &mut buffer[..(length - offset).min(64 * 1024)];
            source.read_exact(bytes)?;
            blob.write_at(bytes, offset)?;
            digest.update(&*bytes);
            offset += bytes.len();
        }
        if source.read(&mut buffer[..1])? != 0 {
            return Err(Error::InvalidData("The attachment length changed."));
        }
        blob.close()?;
        let data_revision = Revision::from_digest(digest.clone());
        digest.update(&meta);
        let revision = Revision::from_digest(digest);
        transaction.execute(
            "UPDATE files SET revision=?,body_revision=? WHERE id=?",
            params![revision.as_str(), data_revision.as_str(), id],
        )?;
        self.register_file(id, SystemTime::now())?;
        transaction.commit()?;
        self.attachment(id)
    }

    pub fn attachment(&self, id: &str) -> Result<Attachment, Error> {
        if !crate::rules::valid_attachment(id.as_bytes()) {
            return Err(Error::InvalidData("The attachment identifier is invalid."));
        }
        let (meta, head, bytes, revision, data_revision) = self
            .index
            .db
            .query_row(
                "SELECT meta,head,bytes,revision,body_revision FROM files WHERE id=?",
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
}
