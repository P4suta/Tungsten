use super::{Note, RecordKind, SavedNote, Store};
use crate::{Error, domain::NoteName};
use rusqlite::{OptionalExtension, params};
use std::time::Duration;

#[derive(serde::Serialize)]
pub struct TrashPage {
    pub entries: Vec<SavedNote>,
    pub next: Option<String>,
}

fn json_error(_: serde_json::Error) -> Error {
    Error::InvalidData("The recovery catalog data is invalid.")
}

impl Store {
    fn record_stamp(&self, kind: RecordKind, id: &str) -> Result<String, Error> {
        self.index
            .db
            .query_row(
                "SELECT revision FROM snapshots WHERE trash=? AND id=?",
                params![kind.is_trash(), id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(Error::NotFound)
    }

    pub(super) fn index_record(
        &self,
        kind: RecordKind,
        id: &str,
        note: &Note,
        captured: u128,
    ) -> Result<SavedNote, Error> {
        let trash = kind.is_trash();
        let saved = SavedNote {
            id: id.to_owned(),
            name: note.name.clone(),
            modified: note.modified,
            revision: note.revision.clone(),
            bytes: note.body.len(),
            captured,
            title: crate::markdown::first_heading(&note.body).map_or_else(
                || {
                    note.body
                        .lines()
                        .find(|line| !line.trim().is_empty())
                        .unwrap_or("Untitled note")
                        .trim()
                        .trim_start_matches('#')
                        .trim()
                        .chars()
                        .take(200)
                        .collect()
                },
                |heading| heading.chars().take(200).collect(),
            ),
        };
        let transaction = if self.index.db.is_autocommit() {
            Some(self.index.db.unchecked_transaction()?)
        } else {
            None
        };
        let sequence = self
            .index
            .db
            .prepare_cached("SELECT seq FROM records WHERE trash=? AND id=?")?
            .query_row(params![trash, id], |row| row.get::<_, i64>(0))
            .optional()?;
        let sequence = match sequence {
            Some(sequence) => sequence,
            None => self.index.db.query_row(
                "UPDATE state SET value=value+1 WHERE key='record-sequence' RETURNING value",
                [],
                |row| row.get::<_, i64>(0),
            )?,
        };
        let summary = serde_json::to_string(&saved).map_err(json_error)?;
        self.index.db.prepare_cached("INSERT INTO records(trash,id,name,captured,summary,stamp,seq) VALUES(?,?,?,?,?,?,?) ON CONFLICT(trash,id) DO UPDATE SET name=excluded.name,captured=excluded.captured,summary=excluded.summary,stamp=excluded.stamp")?.execute(params![trash,id,note.name.as_str(),i64::try_from(captured).map_err(|_|Error::InvalidFile)?,summary,self.record_stamp(kind,id)?,sequence])?;
        if trash {
            self.index.db.execute("INSERT INTO trash_visible SELECT ?1,?2,?3,?4 WHERE ?2>(SELECT value FROM state WHERE key='trash-purge') ON CONFLICT(id) DO UPDATE SET captured=excluded.captured,summary=excluded.summary",params![id,sequence,captured as i64,summary])?;
        }
        let files = self
            .index
            .db
            .prepare_cached("SELECT target FROM record_attachments WHERE trash=? AND record=?")?
            .query_map(params![trash, id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for target in files {
            self.index
                .db
                .execute("INSERT OR IGNORE INTO gc_dirty VALUES(?)", [target])?;
        }
        self.index
            .db
            .prepare_cached("DELETE FROM record_attachments WHERE trash=? AND record=?")?
            .execute(params![trash, id])?;
        for target in crate::markdown::attachment_references(&note.body) {
            self.index
                .db
                .execute("UPDATE gc_files SET unused=NULL WHERE id=?", [&target])?;
            self.index
                .db
                .prepare_cached("INSERT OR IGNORE INTO record_attachments VALUES(?,?,?)")?
                .execute(params![trash, id, target])?;
        }
        if !trash {
            self.index.db.execute(
                "INSERT OR IGNORE INTO retention_queue VALUES(?)",
                [note.name.as_str()],
            )?;
        }
        if let Some(transaction) = transaction {
            transaction.commit()?;
        }
        Ok(saved)
    }

    pub fn trash_page(&self, cursor: Option<&str>) -> Result<TrashPage, Error> {
        let (captured, id) = match cursor.filter(|cursor| !cursor.is_empty()) {
            Some(cursor) => {
                let (captured, id) = cursor
                    .split_once(':')
                    .ok_or(Error::InvalidData("The trash cursor is invalid."))?;
                let captured: i64 = captured
                    .parse()
                    .map_err(|_| Error::InvalidData("The trash cursor is invalid."))?;
                if captured < 0 || !crate::rules::valid_id(id.as_bytes()) {
                    return Err(Error::InvalidData("The trash cursor is invalid."));
                }
                (captured, id)
            }
            None => (i64::MAX, ""),
        };
        let mut entries = self.index.db.prepare_cached("SELECT summary FROM trash_visible WHERE captured=?1 AND id>?2 ORDER BY id LIMIT 51")?
            .query_map(params![captured,id], |row| row.get::<_,String>(0))?
            .map(|row| serde_json::from_str(&row?).map_err(json_error)).collect::<Result<Vec<SavedNote>,Error>>()?;
        if entries.len() < 51 {
            let remaining = 51 - entries.len();
            let older = self.index.db.prepare_cached("SELECT summary FROM trash_visible WHERE captured<?1 ORDER BY captured DESC,id LIMIT ?2")?
                .query_map(params![captured,remaining as i64], |row| row.get::<_,String>(0))?
                .map(|row| serde_json::from_str(&row?).map_err(json_error)).collect::<Result<Vec<SavedNote>,Error>>()?;
            entries.extend(older);
        }
        let next = if entries.len() > 50 {
            entries.truncate(50);
            entries
                .last()
                .map(|entry| format!("{}:{}", entry.captured, entry.id))
        } else {
            None
        };
        Ok(TrashPage { entries, next })
    }

    pub(super) fn indexed_records(
        &self,
        trash: bool,
        name: Option<&NoteName>,
        limit: usize,
    ) -> Result<Vec<SavedNote>, Error> {
        if trash {
            return self
                .index
                .db
                .prepare_cached(
                    "SELECT summary FROM trash_visible ORDER BY captured DESC,id LIMIT ?",
                )?
                .query_map([limit as i64], |row| row.get::<_, String>(0))?
                .map(|row| serde_json::from_str(&row?).map_err(json_error))
                .collect();
        }
        let sql = if name.is_some() {
            "SELECT summary FROM records WHERE trash=?1 AND name=?2 ORDER BY captured DESC,id LIMIT ?3"
        } else {
            "SELECT summary FROM records WHERE trash=?1 ORDER BY captured DESC,id LIMIT ?3"
        };
        let mut statement = self.index.db.prepare_cached(sql)?;
        let mut rows = statement.query(params![trash, name.map(NoteName::as_str), limit as i64])?;
        let mut saved = Vec::new();
        while let Some(row) = rows.next()? {
            saved.push(serde_json::from_str(&row.get::<_, String>(0)?).map_err(json_error)?);
        }
        Ok(saved)
    }

    pub fn process_recovery_cleanup(&mut self, now: u128, limit: usize) -> Result<usize, Error> {
        if !(1..=50).contains(&limit) {
            return Err(Error::InvalidData("The recovery cleanup batch is invalid."));
        }
        let grace = Duration::from_secs(7 * 24 * 60 * 60).as_millis();
        let cutoff = i64::try_from(now.saturating_sub(grace))
            .map_err(|_| Error::InvalidData("The recovery cleanup time is invalid."))?;
        let mut removed = 0;
        let purged=self.index.db.prepare_cached("SELECT id FROM records WHERE trash=1 AND seq<=(SELECT value FROM state WHERE key='trash-purge') ORDER BY seq LIMIT ?")?.query_map([limit as i64],|row|row.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
        for id in purged {
            self.remove_record(RecordKind::Trash, &id)?;
            removed += 1;
        }
        let expired = if now >= grace && removed < limit {
            self.index
                .db
                .prepare_cached(
                    "SELECT trash,id FROM records WHERE captured<=? ORDER BY captured,id LIMIT ?",
                )?
                .query_map(params![cutoff, (limit - removed) as i64], |row| {
                    Ok((row.get::<_, bool>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };
        for (trash, id) in expired {
            if removed >= limit {
                break;
            }
            let kind = if trash {
                RecordKind::Trash
            } else {
                RecordKind::History
            };
            self.record(kind, &id)?;
            if self.captured(kind, &id)? > cutoff as u128 {
                return Err(Error::InvalidFile);
            }
            self.remove_record(kind, &id)?;
            removed += 1;
        }
        if removed < limit {
            let queued = self
                .index
                .db
                .query_row("SELECT name FROM retention_queue LIMIT 1", [], |row| {
                    row.get::<_, String>(0)
                })
                .optional()?;
            if let Some(name) = queued {
                let obsolete=self.index.db.prepare_cached("SELECT id FROM records WHERE trash=0 AND name=? ORDER BY captured DESC,id LIMIT ? OFFSET 50")?.query_map(params![name,(limit-removed) as i64],|row|row.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
                for id in &obsolete {
                    self.remove_record(RecordKind::History, id)?;
                    removed += 1;
                }
                if removed < limit {
                    self.index
                        .db
                        .execute("DELETE FROM retention_queue WHERE name=?", [name])?;
                }
            }
        }
        Ok(removed)
    }

    pub fn empty_trash(&mut self) -> Result<(), Error> {
        let transaction = self.index.db.unchecked_transaction()?;
        transaction.execute("UPDATE state SET value=(SELECT value FROM state WHERE key='record-sequence') WHERE key='trash-purge'",[])?;
        transaction.execute("DELETE FROM trash_visible", [])?;
        transaction.commit()?;
        Ok(())
    }

    pub(super) fn trash_visible(&self, id: &str) -> Result<bool, Error> {
        Ok(self
            .index
            .db
            .query_row("SELECT 1 FROM trash_visible WHERE id=?", [id], |row| {
                row.get::<_, bool>(0)
            })
            .optional()?
            .unwrap_or(false))
    }
}
