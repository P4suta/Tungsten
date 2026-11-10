use crate::{
    Error,
    domain::{NoteName, Revision},
};
use rusqlite::{OptionalExtension, Transaction, params};
use rustix::fs::{self, Mode, OFlags};
use serde::Serialize;
use std::{
    fs::{File, TryLockError},
    io::{self, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

mod backup;
mod catalog;
mod database;
mod file_collection;
mod recovery_catalog;
mod resources;
mod workspaces;
pub use catalog::{Graph, Page};
pub use recovery_catalog::TrashPage;
pub use resources::{
    Attachment, AttachmentMeta, MAX_ATTACHMENT_BYTES, SavedNote, image_type,
    recovery_checkpoint_due,
};
pub use workspaces::{Workspace, WorkspaceEntry, WorkspaceId, WorkspaceNotes, Workspaces};

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

#[derive(Clone, Copy)]
enum RecordKind {
    History,
    Trash,
}

impl RecordKind {
    fn is_trash(self) -> bool {
        matches!(self, Self::Trash)
    }
}

pub struct Store {
    index: catalog::Catalog,
    _lock: DirectoryLock,
}

struct DirectoryLock(File);

impl DirectoryLock {
    fn acquire(file: File) -> Result<Self, Error> {
        file.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => Error::Locked,
            TryLockError::Error(error) => Error::Io(error),
        })?;
        Ok(Self(file))
    }
}

impl Drop for DirectoryLock {
    fn drop(&mut self) {
        if let Err(error) = self.0.unlock() {
            let _ = writeln!(
                io::stderr().lock(),
                "operation=store_lock outcome=release_failed error={error}"
            );
        }
    }
}

fn now() -> Result<i64, Error> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_millis(),
    )
    .map_err(|_| Error::Worker)
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, Error> {
        std::fs::create_dir_all(path)?;
        let root = File::from(
            fs::open(
                path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(io::Error::from)?,
        );
        root.set_permissions(std::fs::Permissions::from_mode(0o700))?;
        let lock = File::from(
            fs::openat(
                &root,
                ".tungsten.lock",
                OFlags::RDWR
                    | OFlags::CREATE
                    | OFlags::NOFOLLOW
                    | OFlags::CLOEXEC
                    | OFlags::NONBLOCK,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(io::Error::from)?,
        );
        if !lock.metadata()?.is_file() || lock.metadata()?.nlink() != 1 {
            return Err(Error::InvalidFile);
        }
        let lock = DirectoryLock::acquire(lock)?;
        for (index, entry) in std::fs::read_dir(path)?.enumerate() {
            let entry = entry?;
            if index >= 8
                || !entry.file_type()?.is_file()
                || entry.metadata()?.nlink() != 1
                || !matches!(
                    entry.file_name().to_str(),
                    Some(
                        "notes.sqlite"
                            | "notes.sqlite-wal"
                            | "notes.sqlite-shm"
                            | "notes.sqlite-journal"
                            | ".tungsten.lock"
                    )
                )
            {
                return Err(Error::InvalidData(
                    "Use an empty directory or a complete notes database.",
                ));
            }
        }
        let index = catalog::Catalog::open(&path.join("notes.sqlite"))?;
        root.sync_all()?;
        Ok(Self { index, _lock: lock })
    }

    fn transaction(&self) -> Result<Option<Transaction<'_>>, Error> {
        if self.index.db.is_autocommit() {
            Ok(Some(self.index.db.unchecked_transaction()?))
        } else {
            Ok(None)
        }
    }

    pub fn read(&self, name: &NoteName) -> Result<Note, Error> {
        let (body, modified) = self
            .index
            .db
            .query_row(
                "SELECT body,modified FROM documents WHERE name=?",
                [name.as_str()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        if body.len() > MAX_NOTE_BYTES {
            return Err(Error::TooLarge);
        }
        Ok(Note {
            name: name.clone(),
            revision: Revision::of(body.as_bytes()),
            body,
            modified: u128::try_from(modified).map_err(|_| Error::InvalidFile)?,
        })
    }

    pub fn read_durable(&self, name: &NoteName) -> Result<Note, Error> {
        self.read(name)
    }

    pub fn list(&self, query: &str) -> Result<Vec<Summary>, Error> {
        Ok(self.search(query, None, 200, false)?.notes)
    }

    pub fn note_count(&self) -> Result<usize, Error> {
        self.index.count()
    }

    pub fn search(
        &self,
        query: &str,
        cursor: Option<&str>,
        limit: usize,
        titles: bool,
    ) -> Result<Page, Error> {
        self.index.search(query, cursor, limit, titles)
    }

    pub fn graph(&self, focus: Option<&NoteName>, limit: usize) -> Result<Graph, Error> {
        self.index.graph(focus, limit)
    }

    pub fn backlinks(&self, name: &NoteName) -> Result<Vec<Summary>, Error> {
        self.index
            .incoming(name, 0, 120)?
            .into_iter()
            .map(|(_, name)| self.index.summary(name.as_str())?.ok_or(Error::NotFound))
            .collect()
    }

    pub fn attachment_sources(&self, id: &str) -> Result<Vec<Summary>, Error> {
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
        if !(1..=256).contains(&limit) {
            return Err(Error::InvalidData("The link update batch is invalid."));
        }
        let job = self.index.db.query_row("SELECT id,target,previous,title,after_id FROM retitles WHERE ready=1 ORDER BY id LIMIT 1", [], |row| Ok((row.get::<_,i64>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?,row.get::<_,i64>(4)?))).optional()?;
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
        let transaction = self.index.db.unchecked_transaction()?;
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
            params![last, id],
        )?;
        if sources.len() < limit && processed == sources.len() {
            self.index
                .db
                .execute("DELETE FROM retitles WHERE id=?", [id])?;
        }
        transaction.commit()?;
        Ok(processed)
    }

    fn refresh_index(&self, name: &NoteName, bulk: bool) -> Result<(), Error> {
        let note = self.read(name)?;
        self.index
            .put(&note, note.revision.as_str(), self.pinned(name)?, bulk)
    }

    pub fn create(&mut self, name: Option<String>, body: &str) -> Result<Note, Error> {
        let name = NoteName::parse(match name {
            Some(name) if !name.is_empty() => name,
            _ => format!("{}.md", Uuid::new_v4().simple()),
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
        let transaction = self.transaction()?;
        let exists: bool = self.index.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM documents WHERE name=?)",
            [name.as_str()],
            |row| row.get(0),
        )?;
        if exists && !replace {
            return Err(Error::Exists);
        }
        self.index.db.execute("INSERT INTO documents VALUES(?,?,?) ON CONFLICT(name) DO UPDATE SET body=excluded.body,modified=excluded.modified", params![name.as_str(), body, now()?])?;
        self.refresh_index(name, false)?;
        if let Some(transaction) = transaction {
            transaction.commit()?;
        }
        Ok(())
    }

    pub fn update(&mut self, name: &NoteName, body: &str, expected: &str) -> Result<Note, Error> {
        if body.len() > MAX_NOTE_BYTES {
            return Err(Error::TooLarge);
        }
        let transaction = self.index.db.unchecked_transaction()?;
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
        transaction.commit()?;
        self.read(name)
    }

    pub fn rename(
        &mut self,
        name: &NoteName,
        renamed: &NoteName,
        expected: &str,
    ) -> Result<Note, Error> {
        let transaction = self.index.db.unchecked_transaction()?;
        let current = self.checked(name, expected)?;
        if name == renamed {
            return Ok(current);
        }
        if self.index.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM documents WHERE name=?)",
            [renamed.as_str()],
            |row| row.get::<_, bool>(0),
        )? {
            return Err(Error::Exists);
        }
        self.index.db.execute(
            "UPDATE documents SET name=? WHERE name=?",
            params![renamed.as_str(), name.as_str()],
        )?;
        self.move_metadata(name, renamed)?;
        self.index.remove(name)?;
        self.refresh_index(renamed, false)?;
        transaction.commit()?;
        self.read(renamed)
    }

    pub fn delete(&mut self, name: &NoteName, expected: &str) -> Result<(), Error> {
        let transaction = self.index.db.unchecked_transaction()?;
        let note = self.checked(name, expected)?;
        self.save_record(RecordKind::Trash, &note)?;
        self.index
            .db
            .execute("DELETE FROM documents WHERE name=?", [name.as_str()])?;
        self.set_pin(name, false)?;
        self.index.remove(name)?;
        transaction.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, Store};

    #[test]
    fn writer_ownership_ends_before_duplicated_descriptors_close() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let duplicate = store._lock.0.try_clone().unwrap();
        assert!(matches!(Store::open(directory.path()), Err(Error::Locked)));
        drop(store);
        let replacement = Store::open(directory.path()).unwrap();
        drop(duplicate);
        assert!(matches!(Store::open(directory.path()), Err(Error::Locked)));
        drop(replacement);
        Store::open(directory.path()).unwrap();
    }
}
