use super::{Note, RecordKind, Store};
use crate::{
    Error,
    backup::{self, Cell, Kind, Reader, Record, StagedDirectory, Writer},
    domain::{NoteName, Revision},
};
use rusqlite::{OptionalExtension, params, params_from_iter, types::Value};
use std::path::Path;

fn cells(row: &rusqlite::Row<'_>, count: usize) -> Result<Vec<Cell>, rusqlite::Error> {
    (0..count)
        .map(|index| {
            Ok(match row.get::<_, Value>(index)? {
                Value::Null => Cell::Null,
                Value::Integer(value) => Cell::Integer(value),
                Value::Text(value) => Cell::Text(value),
                Value::Blob(value) => Cell::Bytes(value),
                Value::Real(_) => return Err(rusqlite::Error::InvalidQuery),
            })
        })
        .collect()
}

fn values(record: &Record) -> Vec<Value> {
    record
        .values
        .iter()
        .map(|cell| match cell {
            Cell::Null => Value::Null,
            Cell::Integer(value) => Value::Integer(*value),
            Cell::Text(value) => Value::Text(value.clone()),
            Cell::Bytes(value) => Value::Blob(value.clone()),
        })
        .collect()
}

impl Store {
    pub fn backup_to(&self, output: &Path) -> Result<(), Error> {
        let source = Path::new(self.index.db.path().ok_or_else(backup::invalid)?)
            .parent()
            .ok_or_else(backup::invalid)?;
        let staging = StagedDirectory::new(output, source)?;
        let mut writer = Writer::new(staging.path())?;
        let transaction = self.index.db.unchecked_transaction()?;
        for kind in Kind::ALL {
            let query = match kind {
                Kind::Document => "SELECT d.name,d.body,d.modified,n.id FROM documents d LEFT JOIN notes n ON n.name=d.name ORDER BY n.id".to_owned(),
                Kind::Snapshot => "SELECT s.trash,s.id,s.name,s.revision,s.modified,s.captured,r.seq,s.body FROM snapshots s LEFT JOIN records r ON r.trash=s.trash AND r.id=s.id ORDER BY r.seq".to_owned(),
                _ => format!("SELECT * FROM {} ORDER BY 1", kind.table()),
            };
            let mut statement = self.index.db.prepare(&query)?;
            let count = statement.column_count();
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                match kind {
                    Kind::Document => {
                        let body: String = row.get(1)?;
                        let record = Record {
                            kind,
                            values: vec![
                                Cell::Text(row.get(0)?),
                                Cell::Text(Revision::of(body.as_bytes()).to_string()),
                                Cell::Integer(row.get(2)?),
                                Cell::Integer(row.get(3)?),
                            ],
                        };
                        writer.record(&record, Some(&mut body.as_bytes()))?;
                    }
                    Kind::Snapshot => {
                        let record = Record {
                            kind,
                            values: cells(row, 7)?,
                        };
                        let body: String = row.get(7)?;
                        writer.record(&record, Some(&mut body.as_bytes()))?;
                    }
                    Kind::File => {
                        let record = Record {
                            kind,
                            values: cells(row, count)?,
                        };
                        let rowid: i64 = self.index.db.query_row(
                            "SELECT rowid FROM file_data WHERE id=?",
                            [record.text(0)?],
                            |row| row.get(0),
                        )?;
                        let mut source =
                            self.index
                                .db
                                .blob_open("main", "file_data", "data", rowid, true)?;
                        writer.record(&record, Some(&mut source))?;
                    }
                    _ => writer.record(
                        &Record {
                            kind,
                            values: cells(row, count)?,
                        },
                        None,
                    )?,
                }
            }
        }
        writer.finish()?;
        transaction.commit()?;
        staging.publish()
    }

    pub fn restore_from(archive: &Path, output: &Path) -> Result<(), Error> {
        let mut reader = Reader::open(archive)?;
        let staging = StagedDirectory::new(output, archive)?;
        let store = Self::open(staging.path())?;
        let transaction = store.index.db.unchecked_transaction()?;
        store.index.db.execute_batch(
            "DELETE FROM state; DELETE FROM workspace_state; DELETE FROM sqlite_sequence;",
        )?;
        while let Some(record) = reader.next_record()? {
            store.restore_record(&reader, &record)?;
        }
        store.rebuild_backup_indexes()?;
        store.index.db.execute_batch(
            "DELETE FROM gc_files; DELETE FROM retention_queue; DELETE FROM gc_dirty; DELETE FROM sqlite_sequence;",
        )?;
        reader.rewind()?;
        while let Some(record) = reader.next_record()? {
            match record.kind {
                Kind::FileClock | Kind::Retention | Kind::DirtyFile => {
                    store.restore_metadata(&record)?
                }
                Kind::TitleUpdate => {
                    store.index.db.execute(
                        "UPDATE retitles SET ready=?2 WHERE id=?1",
                        params![record.integer(0)?, record.integer(6)?],
                    )?;
                }
                Kind::Workspace => {
                    store.index.db.execute(
                        "UPDATE workspaces SET title=?2,revision=?3 WHERE id=?1",
                        params_from_iter(values(&record)),
                    )?;
                }
                Kind::WorkspaceState => {
                    store.index.db.execute(
                        "UPDATE workspace_state SET revision=?2 WHERE id=?1",
                        params_from_iter(values(&record)),
                    )?;
                }
                Kind::Sequence => {
                    let duplicate: bool = store.index.db.query_row(
                        "SELECT EXISTS(SELECT 1 FROM sqlite_sequence WHERE name=?)",
                        [record.text(0)?],
                        |row| row.get(0),
                    )?;
                    if duplicate {
                        return Err(backup::invalid());
                    }
                    store.restore_metadata(&record)?;
                }
                _ => {}
            }
        }
        store.validate_restored_state()?;
        transaction.commit()?;
        let (busy, _, _): (i64, i64, i64) =
            store
                .index
                .db
                .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })?;
        if busy != 0 {
            return Err(backup::invalid());
        }
        drop(store);
        staging.publish()
    }

    fn restore_metadata(&self, record: &Record) -> Result<(), Error> {
        let parameters = vec!["?"; record.values.len()].join(",");
        self.index.db.execute(
            &format!("INSERT INTO {} VALUES({parameters})", record.kind.table()),
            params_from_iter(values(record)),
        )?;
        Ok(())
    }

    fn restore_record(&self, reader: &Reader, record: &Record) -> Result<(), Error> {
        match record.kind {
            Kind::Document | Kind::Snapshot => {
                let mut bytes = Vec::new();
                backup::copy_payload(record, &mut reader.payload(record)?, &mut bytes)?;
                let body = String::from_utf8(bytes).map_err(|_| backup::invalid())?;
                if record.kind == Kind::Document {
                    self.index.db.execute(
                        "INSERT INTO documents VALUES(?,?,?)",
                        params![record.text(0)?, body, record.integer(2)?],
                    )?;
                    self.index.db.execute("INSERT INTO notes(id,name,summary,stamp,title_search,modified,pinned,open_task,tags) VALUES(?,?,'{}',?,'',?,0,0,'[]')", params![record.integer(3)?, record.text(0)?, record.text(1)?, record.integer(2)?])?;
                } else {
                    self.index.db.execute(
                        "INSERT INTO snapshots VALUES(?,?,?,?,?,?,?)",
                        params![
                            record.integer(0)?,
                            record.text(1)?,
                            record.text(2)?,
                            body,
                            record.text(3)?,
                            record.integer(4)?,
                            record.integer(5)?
                        ],
                    )?;
                    self.index.db.execute("INSERT INTO records(trash,id,name,captured,summary,stamp,seq) VALUES(?,?,?,?,'{}',?,?)", params![record.integer(0)?,record.text(1)?,record.text(2)?,record.integer(5)?,record.text(3)?,record.integer(6)?])?;
                }
            }
            Kind::File => {
                self.restore_metadata(record)?;
                self.index.db.execute(
                    "INSERT INTO file_data(id,data) VALUES(?,?)",
                    params![
                        record.text(0)?,
                        rusqlite::blob::ZeroBlob(record.integer(6)? as u64)
                    ],
                )?;
                let mut blob = self.index.db.blob_open(
                    "main",
                    "file_data",
                    "data",
                    self.index.db.last_insert_rowid(),
                    false,
                )?;
                backup::copy_payload(record, &mut reader.payload(record)?, &mut blob)?;
                blob.close()?;
            }
            Kind::Workspace => {
                self.index.db.execute(
                    "INSERT INTO workspaces(id,title,revision) VALUES(?,?,0)",
                    params![record.integer(0)?, record.text(1)?],
                )?;
            }
            Kind::WorkspaceState => {
                self.index.db.execute(
                    "INSERT INTO workspace_state(id,revision) VALUES(?,0)",
                    [record.integer(0)?],
                )?;
            }
            Kind::Sequence => {}
            _ => self.restore_metadata(record)?,
        }
        Ok(())
    }

    fn rebuild_backup_indexes(&self) -> Result<(), Error> {
        let mut after = String::new();
        loop {
            let name = self
                .index
                .db
                .query_row(
                    "SELECT name FROM documents WHERE name>? ORDER BY name LIMIT 1",
                    [&after],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            let Some(name) = name else { break };
            let note = self.read(&NoteName::parse(name.clone())?)?;
            let pinned = self.index.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM pins WHERE name=?)",
                [&name],
                |row| row.get::<_, bool>(0),
            )?;
            self.index
                .put(&note, note.revision.as_str(), pinned, true)?;
            after = name;
        }
        self.index.db.execute_batch("UPDATE notes SET degree=outgoing+(SELECT count(*) FROM links WHERE target=notes.name);")?;
        let mut sequence = 0i64;
        loop {
            let row = self.index.db.query_row("SELECT s.trash,s.id,s.name,s.body,s.revision,s.modified,s.captured,r.seq FROM snapshots s JOIN records r ON r.trash=s.trash AND r.id=s.id WHERE r.seq>? ORDER BY r.seq LIMIT 1", [sequence], |row| Ok((row.get::<_,bool>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?,row.get::<_,String>(4)?,row.get::<_,i64>(5)?,row.get::<_,i64>(6)?,row.get::<_,i64>(7)?))).optional()?;
            let Some((trash, id, name, body, revision, modified, captured, seq)) = row else {
                break;
            };
            let note = Note {
                name: NoteName::parse(name)?,
                body,
                revision: Revision::parse(revision)?,
                modified: modified as u128,
            };
            self.index_record(
                if trash {
                    RecordKind::Trash
                } else {
                    RecordKind::History
                },
                &id,
                &note,
                captured as u128,
            )?;
            sequence = seq;
        }
        Ok(())
    }

    fn validate_restored_state(&self) -> Result<(), Error> {
        let db = &self.index.db;
        let valid: bool = db.query_row("SELECT
          (SELECT value FROM state WHERE key='count')=(SELECT count(*) FROM documents)
          AND (SELECT value FROM state WHERE key='record-sequence')>=coalesce((SELECT max(seq) FROM records),0)
          AND (SELECT value FROM state WHERE key='trash-purge')<=(SELECT value FROM state WHERE key='record-sequence')
          AND EXISTS(SELECT 1 FROM workspace_state WHERE id=1)
          AND NOT EXISTS(SELECT 1 FROM pragma_foreign_key_check)
          AND NOT EXISTS(SELECT 1 FROM pins WHERE name NOT IN(SELECT name FROM documents))
          AND NOT EXISTS(SELECT 1 FROM gc_files WHERE id NOT IN(SELECT id FROM files))", [], |row| row.get(0))?;
        if !valid {
            return Err(backup::invalid());
        }
        for table in ["notes", "retitles", "workspaces", "workspace_members"] {
            let column = if table == "workspace_members" {
                "seq"
            } else {
                "id"
            };
            let valid: bool = db.query_row(&format!("SELECT coalesce((SELECT seq FROM sqlite_sequence WHERE name=?),0)>=coalesce((SELECT max({column}) FROM {table}),0)"), [table], |row| row.get(0))?;
            if !valid {
                return Err(backup::invalid());
            }
        }
        let check: String = db.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if check != "ok" {
            return Err(backup::invalid());
        }
        Ok(())
    }
}
