use super::{Note, RecordKind, Store};
use crate::{
    Error,
    domain::{NoteName, Revision},
    rules::{valid_attachment, valid_id},
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read},
    os::unix::fs::PermissionsExt,
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

pub fn recovery_checkpoint_due(note: &Note, latest: Option<(&Note, u128)>, now: u128) -> bool {
    match latest {
        None => !note.body.trim().is_empty(),
        Some((previous, captured)) => {
            previous.body != note.body
                && (now.saturating_sub(captured) >= Duration::from_secs(5 * 60).as_millis()
                    || now.saturating_sub(note.modified) >= Duration::from_secs(90).as_millis()
                    || significant_edit(&previous.body, &note.body))
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum AttachmentMeta {
    Name(String),
    Details {
        name: String,
        folder: String,
        description: String,
    },
}

impl AttachmentMeta {
    pub fn validate(&self) -> Result<(), Error> {
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
    pub title: String,
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

pub use crate::media::image_type;

impl Store {
    pub fn collect_unused_attachments(&mut self, now: SystemTime) -> Result<usize, Error> {
        self.process_recovery_cleanup(
            now.duration_since(UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_millis(),
            50,
        )?;
        self.collect_indexed_unused_attachments(now, 50)
    }

    pub(super) fn pinned(&self, name: &NoteName) -> Result<bool, Error> {
        Ok(self.index.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM pins WHERE name=?)",
            [name.as_str()],
            |row| row.get(0),
        )?)
    }

    pub(super) fn set_pin(&self, name: &NoteName, pinned: bool) -> Result<(), Error> {
        self.index.db.execute(
            if pinned {
                "INSERT OR IGNORE INTO pins VALUES(?)"
            } else {
                "DELETE FROM pins WHERE name=?"
            },
            [name.as_str()],
        )?;
        Ok(())
    }

    pub fn pin(&mut self, name: &NoteName, pinned: bool, expected: &str) -> Result<(), Error> {
        let transaction = self.index.db.unchecked_transaction()?;
        self.checked(name, expected)?;
        self.set_pin(name, pinned)?;
        self.refresh_index(name, false)?;
        transaction.commit()?;
        Ok(())
    }

    pub(super) fn save_record(&self, kind: RecordKind, note: &Note) -> Result<SavedNote, Error> {
        self.save_record_at(kind, note, super::now()? as u128)
    }

    fn save_record_at(
        &self,
        kind: RecordKind,
        note: &Note,
        captured: u128,
    ) -> Result<SavedNote, Error> {
        let id = Uuid::new_v4().simple().to_string();
        let transaction = self.transaction()?;
        self.index.db.execute(
            "INSERT INTO snapshots VALUES(?,?,?,?,?,?,?)",
            params![
                kind.is_trash(),
                id,
                note.name.as_str(),
                note.body,
                note.revision.as_str(),
                i64::try_from(note.modified).map_err(|_| Error::InvalidFile)?,
                i64::try_from(captured).map_err(|_| Error::InvalidFile)?
            ],
        )?;
        let saved = self.index_record(kind, &id, note, captured)?;
        if let Some(transaction) = transaction {
            transaction.commit()?;
        }
        Ok(saved)
    }

    pub(super) fn record(&self, kind: RecordKind, id: &str) -> Result<Note, Error> {
        if !valid_id(id.as_bytes()) {
            return Err(Error::InvalidData("The record identifier is invalid."));
        }
        self.record_body(id, kind.is_trash())
    }

    pub(super) fn captured(&self, kind: RecordKind, id: &str) -> Result<u128, Error> {
        let captured = self
            .index
            .db
            .query_row(
                "SELECT captured FROM snapshots WHERE trash=? AND id=?",
                params![kind.is_trash(), id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        u128::try_from(captured).map_err(|_| Error::InvalidFile)
    }

    #[cfg(test)]
    fn prune_recovery(&mut self, now: u128) -> Result<(), Error> {
        self.process_recovery_cleanup(now, 50)?;
        Ok(())
    }

    pub(super) fn remove_record(&self, kind: RecordKind, id: &str) -> Result<(), Error> {
        if !valid_id(id.as_bytes()) {
            return Err(Error::InvalidData("The record identifier is invalid."));
        }
        let transaction = self.transaction()?;
        let trash = kind.is_trash();
        let files = self
            .index
            .db
            .prepare_cached("SELECT target FROM record_attachments WHERE trash=? AND record=?")?
            .query_map(params![trash, id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        self.index.db.execute(
            "DELETE FROM snapshots WHERE trash=? AND id=?",
            params![trash, id],
        )?;
        for target in files {
            self.index
                .db
                .execute("INSERT OR IGNORE INTO gc_dirty VALUES(?)", [target])?;
        }
        self.index.db.execute(
            "DELETE FROM records WHERE trash=? AND id=?",
            params![trash, id],
        )?;
        if trash {
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
        self.snapshot_at(note, super::now()? as u128)
    }

    fn snapshot_at(&self, note: &Note, now: u128) -> Result<(), Error> {
        let records = self.history(&note.name)?;
        let latest = records
            .first()
            .map(|record| {
                self.record(RecordKind::History, &record.id)
                    .map(|note| (note, record.captured))
            })
            .transpose()?;
        if !recovery_checkpoint_due(
            note,
            latest.as_ref().map(|(note, captured)| (note, *captured)),
            now,
        ) {
            return Ok(());
        }
        self.save_record_at(RecordKind::History, note, now)?;
        for record in records.iter().skip(49) {
            self.remove_record(RecordKind::History, &record.id)?;
        }
        Ok(())
    }

    pub(super) fn move_metadata(&self, from: &NoteName, to: &NoteName) -> Result<(), Error> {
        for saved in self.history(from)? {
            let mut note = self.record(RecordKind::History, &saved.id)?;
            note.name = to.clone();
            self.index.db.execute(
                "UPDATE snapshots SET name=? WHERE trash=0 AND id=?",
                params![to.as_str(), saved.id],
            )?;
            self.index_record(RecordKind::History, &saved.id, &note, saved.captured)?;
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
        self.record(
            if trash {
                RecordKind::Trash
            } else {
                RecordKind::History
            },
            id,
        )
    }

    pub fn restore_history(
        &mut self,
        id: &str,
        name: &NoteName,
        expected: &str,
    ) -> Result<Note, Error> {
        let note = self.record(RecordKind::History, id)?;
        if &note.name != name {
            return Err(Error::InvalidData("The version belongs to another note."));
        }
        self.update(name, &note.body, expected)
    }

    pub fn trash(&self) -> Result<Vec<SavedNote>, Error> {
        self.indexed_records(true, None, 50)
    }

    pub fn restore_trash(&mut self, id: &str, name: Option<String>) -> Result<Note, Error> {
        let transaction = self.index.db.unchecked_transaction()?;
        let note = self.saved_note(id, true)?;
        let restored = NoteName::parse(name.unwrap_or_else(|| note.name.as_str().to_owned()))?;
        self.write(&restored, &note.body, false)?;
        self.remove_record(RecordKind::Trash, id)?;
        transaction.commit()?;
        self.read(&restored)
    }

    pub fn purge_trash(&mut self, id: &str, expected: &str) -> Result<(), Error> {
        let note = self.record(RecordKind::Trash, id)?;
        if !note.revision.matches(expected) {
            return Err(Error::Conflict);
        }
        self.remove_record(RecordKind::Trash, id)
    }

    pub fn upload(&mut self, name: String, bytes: &[u8]) -> Result<Attachment, Error> {
        self.upload_from(name, bytes.len(), bytes)
    }

    pub fn stage_upload(&self, name: &str) -> Result<std::fs::File, Error> {
        AttachmentMeta::Name(name.to_owned()).validate()?;
        let directory = std::path::Path::new(self.index.db.path().ok_or(Error::InvalidFile)?)
            .parent()
            .ok_or(Error::InvalidFile)?;
        let file = tempfile::tempfile_in(directory)?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        Ok(file)
    }

    pub fn upload_from(
        &mut self,
        name: String,
        length: usize,
        mut source: impl Read,
    ) -> Result<Attachment, Error> {
        if length > MAX_ATTACHMENT_BYTES {
            return Err(Error::Limit("Attachments must be at most 64 MiB."));
        }
        AttachmentMeta::Name(name.clone()).validate()?;
        let mut head = [0; 512];
        let head = &mut head[..length.min(512)];
        source.read_exact(head)?;
        let extension = image_type(head)
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
        self.insert_attachment(
            &id,
            &name,
            length,
            head,
            std::io::Cursor::new(&*head).chain(source),
        )
    }

    pub fn attachment_bytes(&self, id: &str) -> Result<Vec<u8>, Error> {
        if !valid_attachment(id.as_bytes()) {
            return Err(Error::InvalidData("The attachment identifier is invalid."));
        }
        self.index
            .db
            .query_row("SELECT data FROM file_data WHERE id=?", [id], |row| {
                row.get(0)
            })
            .optional()?
            .ok_or(Error::NotFound)
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
        let row = self
            .index
            .db
            .query_row(
                "SELECT rowid,length(data) FROM file_data WHERE id=?",
                [id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        if range.end as u64 > u64::try_from(row.1).map_err(|_| Error::InvalidFile)? {
            return Err(Error::InvalidData("The attachment range is invalid."));
        }
        let mut bytes = vec![0; range.len()];
        let blob = self
            .index
            .db
            .blob_open("main", "file_data", "data", row.0, true)?;
        blob.read_at_exact(&mut bytes, range.start)?;
        Ok(bytes)
    }

    pub fn delete_attachment(&mut self, id: &str, expected: &str) -> Result<(), Error> {
        let transaction = self.index.db.unchecked_transaction()?;
        if !self.attachment(id)?.revision.matches(expected) {
            return Err(Error::Conflict);
        }
        self.index
            .db
            .execute("DELETE FROM files WHERE id=?", [id])?;
        self.index
            .db
            .execute("DELETE FROM gc_files WHERE id=?", [id])?;
        self.index
            .db
            .execute("DELETE FROM gc_dirty WHERE id=?", [id])?;
        transaction.commit()?;
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
        let database = rusqlite::Connection::open(directory.path().join("notes.sqlite")).unwrap();
        let count: i64 = database
            .query_row("SELECT count(*) FROM snapshots WHERE trash=0", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 50);
        drop(store);
        let store = Store::open(directory.path()).unwrap();
        assert_eq!(store.history(&note.name).unwrap().len(), 50);
    }
}
