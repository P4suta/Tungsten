use super::{MAX_NOTE_BYTES, Note, Store, atomic_write, file_error, open_file, subdirectory};
use crate::{
    Error,
    domain::{NoteName, Revision},
    rules::{valid_attachment, valid_id},
};
use rustix::fs::{self, AtFlags, OFlags};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{self, Read, Write},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

pub const MAX_ATTACHMENT_BYTES: usize = 64 * 1024 * 1024;

fn editing_structure(body: &str) -> Vec<String> {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
    let mut result = Vec::new();
    let mut heading = false;
    for event in Parser::new_ext(body, Options::ENABLE_WIKILINKS) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                result.push(format!("heading:{level}"));
                heading = true;
            }
            Event::Text(text) | Event::Code(text) if heading => result.push(text.to_string()),
            Event::End(TagEnd::Heading(_)) => heading = false,
            Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                result.push(format!("link:{dest_url}"))
            }
            _ => {}
        }
    }
    result
}

fn significant_edit(before: &str, after: &str) -> bool {
    let prefix = before
        .bytes()
        .zip(after.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = before.as_bytes()[prefix..]
        .iter()
        .rev()
        .zip(after.as_bytes()[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    before.len() + after.len() - 2 * (prefix + suffix) >= 1024
        || editing_structure(before) != editing_structure(after)
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub(super) enum AttachmentMeta {
    Name(String),
    Details {
        name: String,
        folder: String,
        description: String,
    },
}

impl AttachmentMeta {
    pub(super) fn validate(&self) -> Result<(), Error> {
        let (name, folder, description) = match self {
            Self::Name(name) => (name.as_str(), "", ""),
            Self::Details {
                name,
                folder,
                description,
            } => (name.as_str(), folder.as_str(), description.as_str()),
        };
        if name.is_empty()
            || name.len() > 240
            || name
                .chars()
                .any(|ch| ch.is_control() || matches!(ch, '/' | '\\'))
        {
            return Err(Error::InvalidData("The attachment name is invalid."));
        }
        if !folder.is_empty() {
            NoteName::parse(format!("{folder}/File.md"))?;
        }
        if description.len() > 64 * 1024
            || description
                .chars()
                .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\t' | '\r'))
        {
            return Err(Error::InvalidData("The attachment description is invalid."));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SavedNote {
    pub id: String,
    pub name: NoteName,
    pub modified: u128,
    pub revision: Revision,
    pub bytes: usize,
    pub captured: u128,
    #[serde(default)]
    pub title: String,
}

#[derive(Serialize, Deserialize)]
struct RecordMeta {
    name: String,
    modified: u128,
    revision: String,
    #[serde(default)]
    captured: Option<u128>,
}

#[derive(Debug, Serialize)]
pub struct Attachment {
    pub id: String,
    pub name: String,
    pub bytes: usize,
    pub revision: Revision,
    #[serde(skip)]
    pub data_revision: Revision,
    pub url: String,
    pub image: bool,
    pub kind: crate::media::MediaKind,
    pub mime: &'static str,
    pub folder: String,
    pub description: String,
}

pub(super) fn read_bytes(directory: &File, name: &str, limit: usize) -> Result<Vec<u8>, Error> {
    let file = open_file(directory, name, OFlags::RDONLY | OFlags::NONBLOCK).map_err(file_error)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(Error::InvalidFile);
    }
    if metadata.len() > limit as u64 {
        return Err(Error::Limit("The file is too large."));
    }
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(Error::Limit("The file is too large."));
    }
    Ok(bytes)
}

fn json_error(_: serde_json::Error) -> Error {
    Error::InvalidData("The metadata is not valid JSON.")
}

fn pin_key(name: &NoteName) -> String {
    Revision::of(name.as_str().as_bytes())
        .as_str()
        .trim_matches('"')
        .to_owned()
}

pub use crate::media::image_type;

impl Store {
    pub fn collect_unused_attachments(&mut self, now: SystemTime) -> Result<usize, Error> {
        self.reconcile(true)?;
        self.reconcile_recovery(true)?;
        self.reconcile_files(now)?;
        self.prune_recovery(
            now.duration_since(UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_millis(),
        )?;
        self.collect_indexed_unused_attachments(now, 50)
    }

    pub(super) fn pinned(&self, name: &NoteName) -> Result<bool, Error> {
        if self.encrypted {
            return Ok(self.index.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM vault_pins WHERE name=?)",
                [name.as_str()],
                |row| row.get(0),
            )?);
        }
        match read_bytes(&self.pins, &pin_key(name), 240) {
            Ok(bytes) => Ok(bytes == name.as_str().as_bytes()),
            Err(Error::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub(super) fn set_pin(&self, name: &NoteName, pinned: bool) -> Result<(), Error> {
        if self.encrypted {
            self.index.db.execute(
                if pinned {
                    "INSERT OR IGNORE INTO vault_pins VALUES(?)"
                } else {
                    "DELETE FROM vault_pins WHERE name=?"
                },
                [name.as_str()],
            )?;
            return Ok(());
        }
        let key = pin_key(name);
        if pinned {
            atomic_write(&self.pins, &key, true, |file| {
                file.write_all(name.as_str().as_bytes())
            })?;
        } else {
            match fs::unlinkat(&self.pins, key.as_str(), AtFlags::empty()) {
                Ok(()) | Err(rustix::io::Errno::NOENT) => {}
                Err(error) => return Err(Error::Io(error.into())),
            }
            self.pins.sync_all()?;
        }
        Ok(())
    }

    pub fn pin(&mut self, name: &NoteName, pinned: bool, expected: &str) -> Result<(), Error> {
        self.checked(name, expected)?;
        self.index.journal(&[name])?;
        self.set_pin(name, pinned)?;
        let transaction = self.index.db.unchecked_transaction()?;
        self.refresh_index(name, false)?;
        transaction.commit()?;
        Ok(())
    }

    pub(super) fn save_record(&self, directory: &File, note: &Note) -> Result<SavedNote, Error> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_millis();
        self.save_record_at(directory, note, now)
    }

    fn save_record_at(
        &self,
        directory: &File,
        note: &Note,
        captured: u128,
    ) -> Result<SavedNote, Error> {
        let id = Uuid::new_v4().simple().to_string();
        if self.encrypted {
            let transaction = self.index.db.unchecked_transaction()?;
            transaction.execute(
                "INSERT INTO vault_records VALUES(?,?,?,?,?,?,?)",
                rusqlite::params![
                    std::ptr::eq(directory, &self.trash),
                    id,
                    note.name.as_str(),
                    note.body,
                    note.revision.as_str(),
                    i64::try_from(note.modified).map_err(|_| Error::InvalidFile)?,
                    i64::try_from(captured).map_err(|_| Error::InvalidFile)?
                ],
            )?;
            let saved = self.index_record(directory, &id, note, captured)?;
            transaction.commit()?;
            return Ok(saved);
        }
        let record = subdirectory(directory, &id)?;
        let metadata = RecordMeta {
            name: note.name.as_str().to_owned(),
            modified: note.modified,
            revision: note.revision.as_str().to_owned(),
            captured: Some(captured),
        };
        let json = serde_json::to_vec(&metadata).map_err(json_error)?;
        atomic_write(&record, "metadata.json", false, |file| {
            file.write_all(&json)
        })?;
        atomic_write(&record, "body.md", false, |file| {
            file.write_all(note.body.as_bytes())
        })?;
        self.index_record(directory, &id, note, captured)
    }

    pub(super) fn record(&self, directory: &File, id: &str) -> Result<Note, Error> {
        if !valid_id(id.as_bytes()) {
            return Err(Error::InvalidData("The record identifier is invalid."));
        }
        if self.encrypted {
            return self.encrypted_record(id, std::ptr::eq(directory, &self.trash));
        }
        let record =
            open_file(directory, id, OFlags::RDONLY | OFlags::DIRECTORY).map_err(file_error)?;
        let metadata: RecordMeta =
            serde_json::from_slice(&read_bytes(&record, "metadata.json", 4096)?)
                .map_err(json_error)?;
        let name = NoteName::parse(metadata.name)?;
        let bytes = read_bytes(&record, "body.md", MAX_NOTE_BYTES)?;
        let revision = Revision::of(&bytes);
        if revision.as_str() != metadata.revision {
            return Err(Error::InvalidFile);
        }
        let body = String::from_utf8(bytes).map_err(|_| Error::InvalidFile)?;
        Ok(Note {
            name,
            body,
            revision,
            modified: metadata.modified,
        })
    }

    pub(super) fn captured(&self, directory: &File, id: &str) -> Result<u128, Error> {
        if self.encrypted {
            use rusqlite::OptionalExtension;
            let captured = self
                .index
                .db
                .query_row(
                    "SELECT captured FROM vault_records WHERE trash=? AND id=?",
                    rusqlite::params![std::ptr::eq(directory, &self.trash), id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
                .ok_or(Error::NotFound)?;
            return u128::try_from(captured).map_err(|_| Error::InvalidFile);
        }
        let record =
            open_file(directory, id, OFlags::RDONLY | OFlags::DIRECTORY).map_err(file_error)?;
        let meta: RecordMeta = serde_json::from_slice(&read_bytes(&record, "metadata.json", 4096)?)
            .map_err(json_error)?;
        match meta.captured {
            Some(captured) => Ok(captured),
            None => Ok(record
                .metadata()?
                .modified()?
                .duration_since(UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_millis()),
        }
    }

    fn prune_recovery(&mut self, now: u128) -> Result<(), Error> {
        self.process_recovery_cleanup(now, 50)?;
        Ok(())
    }

    pub(super) fn remove_record(&self, directory: &File, id: &str) -> Result<(), Error> {
        if !valid_id(id.as_bytes()) {
            return Err(Error::InvalidData("The record identifier is invalid."));
        }
        let transaction = if self.encrypted && self.index.db.is_autocommit() {
            Some(self.index.db.unchecked_transaction()?)
        } else {
            None
        };
        let trash = std::ptr::eq(directory, &self.trash);
        let files = self
            .index
            .db
            .prepare_cached("SELECT target FROM record_attachments WHERE trash=? AND record=?")?
            .query_map(rusqlite::params![trash, id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        if self.encrypted {
            self.index.db.execute(
                "DELETE FROM vault_records WHERE trash=? AND id=?",
                rusqlite::params![trash, id],
            )?;
        } else {
            let record =
                open_file(directory, id, OFlags::RDONLY | OFlags::DIRECTORY).map_err(file_error)?;
            for name in ["body.md", "metadata.json"] {
                fs::unlinkat(&record, name, AtFlags::empty()).map_err(io::Error::from)?;
            }
            fs::unlinkat(directory, id, AtFlags::REMOVEDIR).map_err(io::Error::from)?;
            directory.sync_all()?;
        }
        for target in files {
            self.index
                .db
                .execute("INSERT OR IGNORE INTO gc_dirty VALUES(?)", [target])?;
        }
        self.index.db.execute(
            "DELETE FROM records WHERE trash=? AND id=?",
            rusqlite::params![std::ptr::eq(directory, &self.trash), id],
        )?;
        if std::ptr::eq(directory, &self.trash) {
            self.index
                .db
                .execute("DELETE FROM trash_visible WHERE id=?", [id])?;
        }
        if let Some(transaction) = transaction {
            transaction.commit()?;
        }
        Ok(())
    }

    pub(super) fn snapshot(&self, note: &Note) -> Result<(), Error> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_millis();
        self.snapshot_at(note, now)
    }

    fn snapshot_at(&self, note: &Note, now: u128) -> Result<(), Error> {
        let records = self.history(&note.name)?;
        if records.is_empty() && note.body.trim().is_empty() {
            return Ok(());
        }
        if let Some(latest) = records.first() {
            let previous = self.record(&self.history, &latest.id)?;
            if previous.body == note.body {
                return Ok(());
            }
            let continuous = now.saturating_sub(latest.captured)
                < Duration::from_secs(5 * 60).as_millis()
                && now.saturating_sub(note.modified) < Duration::from_secs(90).as_millis();
            if continuous && !significant_edit(&previous.body, &note.body) {
                return Ok(());
            }
        }
        self.save_record_at(&self.history, note, now)?;
        for record in records.iter().skip(49) {
            self.remove_record(&self.history, &record.id)?;
        }
        Ok(())
    }

    pub(super) fn move_metadata(&self, from: &NoteName, to: &NoteName) -> Result<(), Error> {
        for saved in self.history(from)? {
            let mut note = self.record(&self.history, &saved.id)?;
            note.name = to.clone();
            if self.encrypted {
                self.index.db.execute(
                    "UPDATE vault_records SET name=? WHERE trash=0 AND id=?",
                    rusqlite::params![to.as_str(), saved.id],
                )?;
                self.index_record(&self.history, &saved.id, &note, saved.captured)?;
                continue;
            }
            let directory =
                open_file(&self.history, &saved.id, OFlags::RDONLY | OFlags::DIRECTORY)?;
            let meta = RecordMeta {
                name: to.as_str().to_owned(),
                modified: note.modified,
                revision: note.revision.as_str().to_owned(),
                captured: Some(saved.captured),
            };
            let json = serde_json::to_vec(&meta).map_err(json_error)?;
            atomic_write(&directory, "metadata.json", true, |file| {
                file.write_all(&json)
            })?;
            self.index_record(&self.history, &saved.id, &note, saved.captured)?;
        }
        if self.pinned(from)? {
            self.set_pin(to, true)?;
            self.set_pin(from, false)?;
        }
        Ok(())
    }

    pub fn history(&self, name: &NoteName) -> Result<Vec<SavedNote>, Error> {
        self.indexed_records(false, Some(name), 50)
    }

    pub fn saved_note(&self, id: &str, trash: bool) -> Result<Note, Error> {
        if trash && !self.trash_visible(id)? {
            return Err(Error::NotFound);
        }
        self.record(if trash { &self.trash } else { &self.history }, id)
    }

    pub fn restore_history(
        &mut self,
        id: &str,
        name: &NoteName,
        expected: &str,
    ) -> Result<Note, Error> {
        let note = self.record(&self.history, id)?;
        if &note.name != name {
            return Err(Error::InvalidData("The version belongs to another note."));
        }
        self.update(name, &note.body, expected)
    }

    pub fn trash(&self) -> Result<Vec<SavedNote>, Error> {
        self.indexed_records(true, None, 50)
    }

    pub fn restore_trash(&mut self, id: &str, name: Option<String>) -> Result<Note, Error> {
        let note = self.saved_note(id, true)?;
        let restored = self.create(
            Some(name.unwrap_or_else(|| note.name.as_str().to_owned())),
            &note.body,
        )?;
        self.remove_record(&self.trash, id)?;
        Ok(restored)
    }

    pub fn purge_trash(&mut self, id: &str, expected: &str) -> Result<(), Error> {
        let note = self.record(&self.trash, id)?;
        if !note.revision.matches(expected) {
            return Err(Error::Conflict);
        }
        self.remove_record(&self.trash, id)
    }

    pub(super) fn migrate_trash(&self) -> Result<(), Error> {
        if self.encrypted {
            return Ok(());
        }
        for entry in fs::Dir::read_from(&self.trash).map_err(io::Error::from)? {
            let entry = entry.map_err(io::Error::from)?;
            let Ok(raw) = entry.file_name().to_str() else {
                continue;
            };
            let Some((prefix, name)) = raw.split_once('-') else {
                continue;
            };
            if prefix.len() != 12
                || !prefix.bytes().all(|byte| byte.is_ascii_hexdigit())
                || entry.file_type() != fs::FileType::RegularFile
            {
                continue;
            }
            let Ok(name) = NoteName::parse(name.to_owned()) else {
                continue;
            };
            let body = String::from_utf8(read_bytes(&self.trash, raw, MAX_NOTE_BYTES)?)
                .map_err(|_| Error::InvalidFile)?;
            let modified = open_file(&self.trash, raw, OFlags::RDONLY)?
                .metadata()?
                .modified()?
                .duration_since(UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_millis();
            let note = Note {
                name,
                revision: Revision::of(body.as_bytes()),
                body,
                modified,
            };
            self.save_record(&self.trash, &note)?;
            fs::unlinkat(&self.trash, raw, AtFlags::empty()).map_err(io::Error::from)?;
            self.trash.sync_all()?;
        }
        Ok(())
    }

    pub fn upload(&mut self, name: String, bytes: &[u8]) -> Result<Attachment, Error> {
        if bytes.len() > MAX_ATTACHMENT_BYTES {
            return Err(Error::Limit("Attachments must be at most 64 MiB."));
        }
        if name.is_empty()
            || name.len() > 240
            || name
                .chars()
                .any(|ch| ch.is_control() || matches!(ch, '/' | '\\'))
        {
            return Err(Error::InvalidData("The attachment name is invalid."));
        }
        let extension = image_type(bytes)
            .map(|(extension, _)| extension)
            .unwrap_or_else(|| {
                name.rsplit_once('.')
                    .map(|(_, extension)| extension)
                    .filter(|extension| {
                        !extension.is_empty()
                            && extension.len() <= 12
                            && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
                    })
                    .unwrap_or("bin")
            });
        let id = format!("{}.{extension}", Uuid::new_v4().simple());
        if self.encrypted {
            return self.upload_encrypted_attachment(&id, &name, bytes);
        }
        let json = serde_json::to_vec(&name).map_err(json_error)?;
        atomic_write(&self.attachments, &format!("{id}.json"), false, |file| {
            file.write_all(&json)
        })?;
        atomic_write(&self.attachments, &id, false, |file| file.write_all(bytes))?;
        self.register_file(&id, SystemTime::now())?;
        self.attachment(&id)
    }

    pub fn attachment_bytes(&self, id: &str) -> Result<Vec<u8>, Error> {
        if !valid_attachment(id.as_bytes()) {
            return Err(Error::InvalidData("The attachment identifier is invalid."));
        }
        if self.encrypted {
            use rusqlite::OptionalExtension;
            return self
                .index
                .db
                .query_row("SELECT data FROM vault_blobs WHERE id=?", [id], |row| {
                    row.get(0)
                })
                .optional()?
                .ok_or(Error::NotFound);
        }
        read_bytes(&self.attachments, id, MAX_ATTACHMENT_BYTES)
    }

    pub fn attachment_range(
        &self,
        id: &str,
        range: std::ops::Range<usize>,
    ) -> Result<Vec<u8>, Error> {
        if !valid_attachment(id.as_bytes())
            || range.start > range.end
            || range.end > MAX_ATTACHMENT_BYTES
        {
            return Err(Error::InvalidData("The attachment range is invalid."));
        }
        let mut bytes = vec![0; range.len()];
        if self.encrypted {
            use rusqlite::OptionalExtension;
            let row = self
                .index
                .db
                .query_row(
                    "SELECT rowid,length(data) FROM vault_blobs WHERE id=?",
                    [id],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()?
                .ok_or(Error::NotFound)?;
            if range.end as u64 > u64::try_from(row.1).map_err(|_| Error::InvalidFile)? {
                return Err(Error::InvalidData("The attachment range is invalid."));
            }
            let blob = self
                .index
                .db
                .blob_open("main", "vault_blobs", "data", row.0, true)?;
            blob.read_at_exact(&mut bytes, range.start)?;
        } else {
            use std::io::{Seek, SeekFrom};
            let mut file = open_file(&self.attachments, id, OFlags::RDONLY | OFlags::NONBLOCK)
                .map_err(file_error)?;
            let metadata = file.metadata()?;
            if !metadata.is_file()
                || metadata.len() > MAX_ATTACHMENT_BYTES as u64
                || range.end as u64 > metadata.len()
            {
                return Err(Error::InvalidFile);
            }
            file.seek(SeekFrom::Start(range.start as u64))?;
            file.read_exact(&mut bytes)?;
        }
        Ok(bytes)
    }

    pub fn attachment(&self, id: &str) -> Result<Attachment, Error> {
        if self.encrypted {
            return self.encrypted_attachment(id);
        }
        let bytes = self.attachment_bytes(id)?;
        let meta_bytes = match read_bytes(&self.attachments, &format!("{id}.json"), 512 * 1024) {
            Ok(meta) => meta,
            Err(Error::NotFound) => serde_json::to_vec(id).map_err(json_error)?,
            Err(error) => return Err(error),
        };
        let meta: AttachmentMeta = serde_json::from_slice(&meta_bytes).map_err(json_error)?;
        meta.validate()?;
        let (name, folder, description) = match meta {
            AttachmentMeta::Name(name) => (name, String::new(), String::new()),
            AttachmentMeta::Details {
                name,
                folder,
                description,
            } => (name, folder, description),
        };
        let (kind, mime) = crate::media::content_type(id, &bytes);
        let data_revision = Revision::of(&bytes);
        let revision = Revision::of_parts(&[&bytes, &meta_bytes]);
        Ok(Attachment {
            id: id.to_owned(),
            name,
            bytes: bytes.len(),
            revision,
            data_revision,
            url: format!("/.attachments/{id}"),
            image: image_type(&bytes).is_some(),
            kind,
            mime,
            folder,
            description,
        })
    }

    pub fn delete_attachment(&mut self, id: &str, expected: &str) -> Result<(), Error> {
        let attachment = self.attachment(id)?;
        if !attachment.revision.matches(expected) {
            return Err(Error::Conflict);
        }
        if self.encrypted {
            self.index
                .db
                .execute("DELETE FROM vault_files WHERE id=?", [id])?;
        } else {
            fs::unlinkat(&self.attachments, id, AtFlags::empty()).map_err(io::Error::from)?;
            match fs::unlinkat(&self.attachments, format!("{id}.json"), AtFlags::empty()) {
                Ok(()) | Err(rustix::io::Errno::NOENT) => {}
                Err(error) => return Err(Error::Io(error.into())),
            }
            self.attachments.sync_all()?;
        }
        self.index
            .db
            .execute("DELETE FROM gc_files WHERE id=?", [id])?;
        self.index
            .db
            .execute("DELETE FROM gc_dirty WHERE id=?", [id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoints_keep_session_starts_and_expire_by_capture_time() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(directory.path()).unwrap();
        let mut note = store.create(Some("Note.md".to_owned()), "first").unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        store.snapshot_at(&note, now).unwrap();
        let first = store.history(&note.name).unwrap().remove(0);
        note.body = "second".to_owned();
        note.revision = Revision::of(note.body.as_bytes());
        store.snapshot_at(&note, now + 60_000).unwrap();
        let second = store.history(&note.name).unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].id, first.id);
        assert_eq!(second[0].captured, now);
        assert!(store.saved_note(&first.id, false).is_ok());
        assert_eq!(
            store.saved_note(&second[0].id, false).unwrap().body,
            "first"
        );
        store.snapshot_at(&note, now + 300_000).unwrap();
        assert_eq!(store.history(&note.name).unwrap().len(), 2);
        let renamed = NoteName::parse("Renamed.md".to_owned()).unwrap();
        let current = store.read(&note.name).unwrap();
        store
            .rename(&note.name, &renamed, current.revision.as_str())
            .unwrap();
        assert_eq!(store.history(&renamed).unwrap()[0].captured, now + 300_000);
        let week = Duration::from_secs(7 * 24 * 60 * 60).as_millis();
        store.prune_recovery(now + week).unwrap();
        assert_eq!(store.history(&renamed).unwrap().len(), 1);
        store.prune_recovery(now + week + 300_000).unwrap();
        assert!(store.history(&renamed).unwrap().is_empty());
    }

    #[test]
    fn recovery_sessions_preserve_small_edits_and_checkpoint_idle_or_structural_boundaries() {
        assert!(!significant_edit(
            "```md\n# Literal\n```",
            "```md\n# Changed\n```"
        ));
        assert!(significant_edit("# Title", "# Renamed"));
        assert!(significant_edit(
            "[Reference](Before.md)",
            "[Reference](After.md)"
        ));
        assert!(significant_edit("", &"x".repeat(1024)));
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let mut note = Note {
            name: NoteName::parse("Sessions.md".to_owned()).unwrap(),
            body: "Before".into(),
            revision: Revision::of(b"Before"),
            modified: 1000,
        };
        store.snapshot_at(&note, 1000).unwrap();
        note.body = "Small edit".into();
        note.revision = Revision::of(note.body.as_bytes());
        note.modified = 2000;
        store.snapshot_at(&note, 3000).unwrap();
        assert_eq!(store.history(&note.name).unwrap().len(), 1);
        store.snapshot_at(&note, 92_000).unwrap();
        assert_eq!(store.history(&note.name).unwrap().len(), 2);
        note.body = "# A section".into();
        note.revision = Revision::of(note.body.as_bytes());
        note.modified = 92_001;
        store.snapshot_at(&note, 92_002).unwrap();
        assert_eq!(store.history(&note.name).unwrap().len(), 3);
        store.snapshot_at(&note, 999_999).unwrap();
        assert_eq!(store.history(&note.name).unwrap().len(), 3);
    }

    #[test]
    fn checkpoint_limits_and_existing_metadata_keep_recovery_bounded() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(directory.path()).unwrap();
        let mut note = store.create(Some("Note.md".to_owned()), "source").unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        for index in 0..55 {
            note.body = format!("source {index}");
            note.revision = Revision::of(note.body.as_bytes());
            store.snapshot_at(&note, now + index * 300_000).unwrap();
        }
        let records = store.history(&note.name).unwrap();
        assert_eq!(records.len(), 50);
        let metadata = directory
            .path()
            .join(format!(".history/{}/metadata.json", records[0].id));
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&metadata).unwrap()).unwrap();
        value.as_object_mut().unwrap().remove("captured");
        std::fs::write(metadata, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(store.history(&note.name).unwrap().len(), 50);
    }
}
