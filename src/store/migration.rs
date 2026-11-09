use super::{Store, encrypted::EncryptedDirectory, open_file, resources::read_bytes};
use crate::{
    Error,
    domain::{NoteName, Revision},
};
use rusqlite::{Connection, OptionalExtension, params};
use rustix::fs::OFlags;
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::MetadataExt,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use zeroize::Zeroizing;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum CopyPhase {
    Catalog,
    Payload,
}

#[derive(Default)]
pub(crate) struct MigrationStop {
    stopped: AtomicBool,
    interrupt: Mutex<Option<rusqlite::InterruptHandle>>,
}

impl MigrationStop {
    pub(crate) fn cancel(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        if let Ok(handle) = self.interrupt.lock()
            && let Some(handle) = &*handle
        {
            handle.interrupt();
        }
    }

    fn check(&self) -> Result<(), Error> {
        if self.stopped.load(Ordering::Relaxed) {
            Err(Error::Worker)
        } else {
            Ok(())
        }
    }
}

fn raw_key(key: &[u8; 32]) -> Zeroizing<String> {
    use std::fmt::Write;
    let mut text = Zeroizing::new(String::from("x'"));
    for byte in key {
        write!(&mut *text, "{byte:02x}").expect("string write");
    }
    text.push('\'');
    text
}

fn snapshot(source: &Store) -> Result<String, Error> {
    let mut digest = Sha256::new();
    for query in [
        "SELECT json_array(name,stamp,pinned,modified) FROM notes ORDER BY name",
        "SELECT json_array(trash,id,name,captured,stamp,seq) FROM records ORDER BY trash,id",
        "SELECT json_array(id,seq,captured,summary) FROM trash_visible ORDER BY id",
        "SELECT json_array(id,stamp,unused) FROM gc_files ORDER BY id",
        "SELECT json_array(id) FROM gc_dirty ORDER BY id",
        "SELECT json_array(name) FROM retention_queue ORDER BY name",
        "SELECT json_array(id,target,previous,title,revision,after_id,ready) FROM retitles ORDER BY id",
        "SELECT json_array(key,value) FROM state WHERE key IN ('count','record-sequence','trash-purge') ORDER BY key",
    ] {
        digest.update(query.as_bytes());
        let mut statement = source.index.db.prepare(query)?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let value: String = row.get(0)?;
            digest.update((value.len() as u64).to_le_bytes());
            digest.update(value.as_bytes());
        }
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

impl EncryptedDirectory {
    pub(crate) fn migrate(
        &self,
        source: &Path,
        key: &[u8; 32],
        stop: Arc<MigrationStop>,
        mut progress: impl FnMut(CopyPhase) -> Result<(), Error>,
    ) -> Result<Store, Error> {
        stop.check()?;
        let mut source = Store::open(source)?;
        *stop.interrupt.lock().map_err(|_| Error::Worker)? =
            Some(source.index.db.get_interrupt_handle());
        let monitor = Arc::clone(&stop);
        source
            .index
            .db
            .progress_handler(2000, Some(move || monitor.stopped.load(Ordering::Relaxed)))?;
        let mut progress = |phase| {
            stop.check()?;
            progress(phase)
        };
        stop.check()?;
        source.reconcile(true)?;
        source.reconcile_recovery(true)?;
        let meta = source.root.metadata()?;
        let fingerprint = snapshot(&source)?;
        let identity = format!("{}:{}:{fingerprint}", meta.dev(), meta.ino());
        let path = self.path.join(".sealed.sqlite");
        if path.try_exists()? {
            let db = Connection::open_with_flags(
                &path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                    | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
            )?;
            db.pragma_update(None, "key", raw_key(key).as_str())?;
            let has_state: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='state')",
                [],
                |row| row.get(0),
            )?;
            let exported = if has_state {
                db.query_row(
                    "SELECT value FROM state WHERE key='migration-source'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
            } else {
                None
            };
            match exported {
                Some(saved) if saved == identity => {}
                Some(_) => {
                    return Err(Error::InvalidData(
                        "Resume migration with the original source directory.",
                    ));
                }
                None => {
                    drop(db);
                    for name in [
                        ".sealed.sqlite",
                        ".sealed.sqlite-wal",
                        ".sealed.sqlite-shm",
                        ".sealed.sqlite-journal",
                    ] {
                        match std::fs::remove_file(self.path.join(name)) {
                            Ok(()) => {}
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                            Err(error) => return Err(error.into()),
                        }
                    }
                    self.root.sync_all()?;
                }
            }
        }
        if !path.try_exists()? {
            let destination = path.to_str().ok_or(Error::InvalidFile)?;
            source.index.db.execute(
                "ATTACH DATABASE ? AS migrated KEY ?",
                params![destination, raw_key(key).as_str()],
            )?;
            let exported = (|| -> Result<(), Error> {
                source
                    .index
                    .db
                    .execute_batch("PRAGMA migrated.auto_vacuum=2;")?;
                source
                    .index
                    .db
                    .query_row("SELECT sqlcipher_export('migrated')", [], |_| Ok(()))?;
                progress(CopyPhase::Catalog)?;
                source.index.db.execute_batch(
                    "PRAGMA migrated.application_id=1414745172; PRAGMA migrated.user_version=6;",
                )?;
                source.index.db.execute(
                    "INSERT OR REPLACE INTO migrated.state VALUES('migration-source',?)",
                    [&identity],
                )?;
                Ok(())
            })();
            let detached = source.index.db.execute_batch("DETACH DATABASE migrated");
            exported?;
            detached?;
            self.root.sync_all()?;
        }
        let target = self.store(key)?;
        target.index.db.execute_batch(
            "INSERT OR IGNORE INTO vault_pins SELECT name FROM notes WHERE pinned=1;",
        )?;
        let count = |store: &Store, table: &str| -> Result<i64, Error> {
            Ok(store
                .index
                .db
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })?)
        };
        for table in ["notes", "records", "gc_files"] {
            if count(&source, table)? != count(&target, table)? {
                return Err(Error::InvalidData(
                    "The migration source changed. Keep both copies and restart with a fresh destination.",
                ));
            }
        }
        loop {
            let cursor: String = target.index.db.query_row(
                "SELECT coalesce(max(name),'') FROM vault_notes",
                [],
                |row| row.get(0),
            )?;
            let names = target
                .index
                .db
                .prepare("SELECT name,stamp FROM notes WHERE name>? ORDER BY name LIMIT 100")?
                .query_map([cursor], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            if names.is_empty() {
                break;
            }
            let transaction = target.index.db.unchecked_transaction()?;
            for (name, stamp) in names {
                let name = NoteName::parse(name)?;
                if source.stamp(&name)? != stamp {
                    return Err(Error::Conflict);
                }
                let note = source.read(&name)?;
                if source.stamp(&name)? != stamp {
                    return Err(Error::Conflict);
                }
                transaction.execute(
                    "INSERT INTO vault_notes VALUES(?,?,?)",
                    params![
                        name.as_str(),
                        note.body,
                        i64::try_from(note.modified).map_err(|_| Error::InvalidFile)?
                    ],
                )?;
            }
            transaction.commit()?;
            progress(CopyPhase::Payload)?;
        }
        for trash in [false, true] {
            let from = if trash {
                &source.trash
            } else {
                &source.history
            };
            loop {
                let cursor: String = target.index.db.query_row(
                    "SELECT coalesce(max(id),'') FROM vault_records WHERE trash=?",
                    [trash],
                    |row| row.get(0),
                )?;
                let records = target
                    .index
                    .db
                    .prepare(
                        "SELECT id,stamp FROM records WHERE trash=? AND id>? ORDER BY id LIMIT 100",
                    )?
                    .query_map(params![trash, cursor], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                if records.is_empty() {
                    break;
                }
                let transaction = target.index.db.unchecked_transaction()?;
                for (id, stamp) in records {
                    if source.record_stamp(from, &id)? != stamp {
                        return Err(Error::Conflict);
                    }
                    let note = source.record(from, &id)?;
                    let captured = source.captured(from, &id)?;
                    if source.record_stamp(from, &id)? != stamp {
                        return Err(Error::Conflict);
                    }
                    transaction.execute(
                        "INSERT INTO vault_records VALUES(?,?,?,?,?,?,?)",
                        params![
                            trash,
                            id,
                            note.name.as_str(),
                            note.body,
                            note.revision.as_str(),
                            i64::try_from(note.modified).map_err(|_| Error::InvalidFile)?,
                            i64::try_from(captured).map_err(|_| Error::InvalidFile)?
                        ],
                    )?;
                }
                transaction.commit()?;
                progress(CopyPhase::Payload)?;
            }
        }
        loop {
            let cursor: String = target.index.db.query_row(
                "SELECT coalesce(max(id),'') FROM vault_files",
                [],
                |row| row.get(0),
            )?;
            let next: Option<(String, String)> = target
                .index
                .db
                .query_row(
                    "SELECT id,stamp FROM gc_files WHERE id>? ORDER BY id LIMIT 1",
                    [cursor],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let Some((id, stamp)) = next else { break };
            if source.attachment_stamp(&id)? != stamp {
                return Err(Error::Conflict);
            }
            let file = source.attachment(&id)?;
            let bytes = source.attachment_bytes(&id)?;
            let meta = match read_bytes(&source.attachments, &format!("{id}.json"), 512 * 1024) {
                Ok(meta) => meta,
                Err(Error::NotFound) => serde_json::to_vec(&id).map_err(|_| Error::InvalidFile)?,
                Err(error) => return Err(error),
            };
            if source.attachment_stamp(&id)? != stamp
                || Revision::of_parts(&[&bytes, &meta]) != file.revision
            {
                return Err(Error::Conflict);
            }
            let transaction = target.index.db.unchecked_transaction()?;
            transaction.execute(
                "INSERT INTO vault_files VALUES(?,?,?,?,?,?,?)",
                params![
                    id,
                    &meta,
                    &bytes[..bytes.len().min(512)],
                    file.revision.as_str(),
                    Revision::of(&bytes).as_str(),
                    0,
                    bytes.len() as i64
                ],
            )?;
            transaction.execute(
                "INSERT INTO vault_blobs(id,data) VALUES(?,?)",
                params![id, &bytes],
            )?;
            transaction.commit()?;
            progress(CopyPhase::Payload)?;
        }
        for (catalog, payload) in [
            ("notes", "vault_notes"),
            ("records", "vault_records"),
            ("gc_files", "vault_files"),
        ] {
            if count(&target, catalog)? != count(&target, payload)? {
                return Err(Error::InvalidData("The migrated payload is incomplete."));
            }
        }
        let mut cursor = String::new();
        loop {
            let names = target
                .index
                .db
                .prepare("SELECT name FROM vault_notes WHERE name>? ORDER BY name LIMIT 100")?
                .query_map([&cursor], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            if names.is_empty() {
                break;
            }
            let transaction = target.index.db.unchecked_transaction()?;
            for name in names {
                let parsed = NoteName::parse(name.clone())?;
                let original = source.read(&parsed)?;
                let copied = target.read(&parsed)?;
                if original.revision != copied.revision || original.modified != copied.modified {
                    return Err(Error::Conflict);
                }
                transaction.execute(
                    "UPDATE notes SET stamp=?,search='' WHERE name=?",
                    params![copied.revision.as_str(), &name],
                )?;
                cursor = name;
            }
            transaction.commit()?;
            progress(CopyPhase::Payload)?;
        }
        for trash in [false, true] {
            let from = if trash {
                &source.trash
            } else {
                &source.history
            };
            let mut cursor = String::new();
            loop {
                let ids = target
                    .index
                    .db
                    .prepare(
                        "SELECT id FROM vault_records WHERE trash=? AND id>? ORDER BY id LIMIT 100",
                    )?
                    .query_map(params![trash, cursor], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                if ids.is_empty() {
                    break;
                }
                let transaction = target.index.db.unchecked_transaction()?;
                for id in ids {
                    let original = source.record(from, &id)?;
                    let copied = target.encrypted_record(&id, trash)?;
                    if original.name != copied.name
                        || original.revision != copied.revision
                        || original.modified != copied.modified
                        || source.captured(from, &id)?
                            != target.captured(
                                if trash {
                                    &target.trash
                                } else {
                                    &target.history
                                },
                                &id,
                            )?
                    {
                        return Err(Error::Conflict);
                    }
                    transaction.execute(
                        "UPDATE records SET stamp=? WHERE trash=? AND id=?",
                        params![copied.revision.as_str(), trash, &id],
                    )?;
                    cursor = id;
                }
                transaction.commit()?;
                progress(CopyPhase::Payload)?;
            }
        }
        let mut cursor = String::new();
        loop {
            let next: Option<String> = target
                .index
                .db
                .query_row(
                    "SELECT id FROM vault_files WHERE id>? ORDER BY id LIMIT 1",
                    [&cursor],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(id) = next else { break };
            let original = source.attachment(&id)?;
            let copied = target.attachment(&id)?;
            if original.revision != copied.revision
                || original.data_revision != Revision::of(&target.attachment_bytes(&id)?)
            {
                return Err(Error::Conflict);
            }
            target.index.db.execute(
                "UPDATE gc_files SET stamp=? WHERE id=?",
                params![copied.revision.as_str(), &id],
            )?;
            cursor = id;
            progress(CopyPhase::Payload)?;
        }
        if snapshot(&source)? != fingerprint {
            return Err(Error::Conflict);
        }
        target
            .index
            .db
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        open_file(&self.root, ".sealed.sqlite", OFlags::RDONLY)?.sync_all()?;
        self.root.sync_all()?;
        Ok(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_and_changed_visibility_never_publish_an_incomplete_copy() {
        for cancel in [true, false] {
            let source = tempfile::tempdir().unwrap();
            let target = tempfile::tempdir().unwrap();
            let key = [117; 32];
            let mut store = Store::open(source.path()).unwrap();
            let note = store
                .create(Some("source.md".into()), "# Original body")
                .unwrap();
            store.pin(&note.name, true, note.revision.as_str()).unwrap();
            drop(store);
            let directory = EncryptedDirectory::open(target.path()).unwrap();
            let stop = Arc::new(MigrationStop::default());
            let signal = Arc::clone(&stop);
            let result = directory.migrate(source.path(), &key, stop, |phase| {
                if phase == CopyPhase::Payload {
                    if cancel {
                        signal.cancel();
                        Ok(())
                    } else {
                        Err(Error::Worker)
                    }
                } else {
                    Ok(())
                }
            });
            assert!(result.is_err());
            if !cancel {
                let mut original = Store::open(source.path()).unwrap();
                original
                    .pin(&note.name, false, note.revision.as_str())
                    .unwrap();
                drop(original);
                assert!(
                    directory
                        .migrate(source.path(), &key, Arc::default(), |_| Ok(()))
                        .is_err()
                );
            } else {
                let resumed = directory
                    .migrate(source.path(), &key, Arc::default(), |_| Ok(()))
                    .unwrap();
                assert_eq!(resumed.read(&note.name).unwrap().body, note.body);
            }
        }
    }

    #[test]
    fn resumes_catalog_export_before_the_marker_and_before_any_schema_exists() {
        for empty_database in [false, true] {
            let source = tempfile::tempdir().unwrap();
            let target = tempfile::tempdir().unwrap();
            let key = [107; 32];
            let mut original = Store::open(source.path()).unwrap();
            let note = original
                .create(Some("original.md".into()), "# Original body")
                .unwrap();
            drop(original);
            let destination = EncryptedDirectory::open(target.path()).unwrap();
            if empty_database {
                let database = Connection::open(target.path().join(".sealed.sqlite")).unwrap();
                database
                    .pragma_update(None, "key", raw_key(&key).as_str())
                    .unwrap();
                database.pragma_update(None, "application_id", 0).unwrap();
                drop(database);
            } else {
                assert!(
                    destination
                        .migrate(source.path(), &key, Arc::default(), |phase| {
                            if phase == CopyPhase::Catalog {
                                Err(Error::Worker)
                            } else {
                                Ok(())
                            }
                        })
                        .is_err()
                );
            }
            let copied = destination
                .migrate(source.path(), &key, Arc::default(), |_| Ok(()))
                .unwrap();
            assert_eq!(copied.read(&note.name).unwrap().body, note.body);
            assert_eq!(copied.list("original").unwrap().len(), 1);
        }
    }

    #[test]
    fn refuses_a_changed_source_after_a_committed_batch() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let key = [103; 32];
        let mut store = Store::open(source.path()).unwrap();
        for index in 0..110 {
            store
                .create(Some(format!("{index:03}.md")), "# Original body")
                .unwrap();
        }
        drop(store);
        let destination = EncryptedDirectory::open(target.path()).unwrap();
        assert!(
            destination
                .migrate(source.path(), &key, Arc::default(), |phase| {
                    if phase == CopyPhase::Payload {
                        Err(Error::Worker)
                    } else {
                        Ok(())
                    }
                })
                .is_err()
        );
        std::fs::write(
            source.path().join("000.md"),
            "# Changed while transfer stopped",
        )
        .unwrap();
        assert!(
            destination
                .migrate(source.path(), &key, Arc::default(), |_| Ok(()))
                .is_err()
        );
        assert!(source.path().join("000.md").exists());
    }

    #[test]
    fn resumes_bounded_copies_without_changing_originals_or_recovery_visibility() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let key = [101; 32];
        let mut store = Store::open(source.path()).unwrap();
        let file = store
            .upload("original.txt".into(), b"Exact original bytes")
            .unwrap();
        std::fs::write(
            source
                .path()
                .join(".attachments")
                .join(format!("{}.json", file.id)),
            br#"{"name":"original.txt","folder":"Reading","description":"Preserve metadata"}"#,
        )
        .unwrap();
        let file = store.attachment(&file.id).unwrap();
        let note = store
            .create(
                Some("retained.md".into()),
                &format!("# Retained\n\n[file]({})", file.url),
            )
            .unwrap();
        store.pin(&note.name, true, note.revision.as_str()).unwrap();
        let changed = store
            .update(
                &note.name,
                "# Changed\n\nSaved body",
                note.revision.as_str(),
            )
            .unwrap();
        let removed = store
            .create(Some("trash.md".into()), "# Deleted note")
            .unwrap();
        store
            .delete(&removed.name, removed.revision.as_str())
            .unwrap();
        let history = store.history(&note.name).unwrap();
        let trash = store.trash().unwrap();
        drop(store);
        let destination = EncryptedDirectory::open(target.path()).unwrap();
        assert!(
            destination
                .migrate(source.path(), &key, Arc::default(), |phase| {
                    if phase == CopyPhase::Payload {
                        Err(Error::Worker)
                    } else {
                        Ok(())
                    }
                })
                .is_err()
        );
        let copied = destination
            .migrate(source.path(), &key, Arc::default(), |_| Ok(()))
            .unwrap();
        assert_eq!(copied.read(&note.name).unwrap().body, changed.body);
        assert_eq!(copied.list("saved").unwrap().len(), 1);
        assert_eq!(copied.list("body").unwrap().len(), 1);
        assert!(
            copied
                .list("")
                .unwrap()
                .iter()
                .find(|entry| entry.name == note.name)
                .unwrap()
                .pinned
        );
        assert_eq!(copied.history(&note.name).unwrap()[0].id, history[0].id);
        assert_eq!(
            copied.saved_note(&history[0].id, false).unwrap().body,
            note.body
        );
        assert_eq!(copied.trash().unwrap()[0].id, trash[0].id);
        assert_eq!(
            copied.attachment_bytes(&file.id).unwrap(),
            b"Exact original bytes"
        );
        assert_eq!(copied.attachment(&file.id).unwrap().revision, file.revision);
        assert_eq!(copied.attachment(&file.id).unwrap().folder, "Reading");
        assert_eq!(
            copied.attachment(&file.id).unwrap().description,
            "Preserve metadata"
        );
        drop(copied);
        let original = Store::open(source.path()).unwrap();
        assert_eq!(original.read(&note.name).unwrap().body, changed.body);
        assert_eq!(original.trash().unwrap()[0].id, trash[0].id);
    }
}
