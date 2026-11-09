use super::{Store, file_error, open_file, resources::read_bytes};
use crate::{Error, rules::valid_attachment};
use rusqlite::{OptionalExtension, params};
use rustix::fs::{self, AtFlags, OFlags};
use std::{
    collections::{BTreeMap, HashSet},
    io,
    os::unix::fs::MetadataExt,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn millis(now: SystemTime) -> Result<i64, Error> {
    i64::try_from(
        now.duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_millis(),
    )
    .map_err(|_| Error::InvalidData("The cleanup time is invalid."))
}

impl Store {
    fn file_referenced(&self, id: &str) -> Result<bool, Error> {
        Ok(self.index.db.prepare_cached("SELECT EXISTS(SELECT 1 FROM attachments WHERE target=?1 UNION ALL SELECT 1 FROM record_attachments WHERE target=?1 LIMIT 1)")?.query_row([id],|row|row.get(0))?)
    }

    pub(super) fn attachment_stamp(&self, id: &str) -> Result<String, Error> {
        if self.encrypted {
            return Ok(self
                .index
                .db
                .query_row("SELECT revision FROM vault_files WHERE id=?", [id], |row| {
                    row.get(0)
                })
                .optional()?
                .unwrap_or_else(|| "missing;".into()));
        }
        let mut stamp = String::new();
        for name in [id.to_owned(), format!("{id}.json")] {
            match open_file(&self.attachments, &name, OFlags::RDONLY | OFlags::NONBLOCK)
                .map_err(file_error)
            {
                Ok(file) => {
                    let meta = file.metadata()?;
                    if !meta.is_file() {
                        return Err(Error::InvalidFile);
                    }
                    use std::fmt::Write;
                    let _ = write!(
                        stamp,
                        "{}:{}:{}:{}:{}:{};",
                        meta.ino(),
                        meta.len(),
                        meta.mtime(),
                        meta.mtime_nsec(),
                        meta.ctime(),
                        meta.ctime_nsec()
                    );
                }
                Err(Error::NotFound) => stamp.push_str("missing;"),
                Err(error) => return Err(error),
            }
        }
        Ok(stamp)
    }

    pub(super) fn register_file(&self, id: &str, now: SystemTime) -> Result<(), Error> {
        if !valid_attachment(id.as_bytes()) {
            return Err(Error::InvalidData("The attachment identifier is invalid."));
        }
        let stamp = self.attachment_stamp(id)?;
        let unused = if self.file_referenced(id)? {
            None
        } else {
            Some(millis(now)?)
        };
        self.index.db.prepare_cached("INSERT INTO gc_files VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET stamp=excluded.stamp,unused=CASE WHEN gc_files.stamp!=excluded.stamp THEN excluded.unused ELSE coalesce(gc_files.unused,excluded.unused) END")?.execute(params![id,stamp,unused])?;
        if unused.is_none() {
            self.index
                .db
                .prepare_cached("UPDATE gc_files SET unused=NULL WHERE id=?")?
                .execute([id])?;
        }
        Ok(())
    }

    pub(super) fn reconcile_files(&self, now: SystemTime) -> Result<(), Error> {
        if self.encrypted {
            return Ok(());
        }
        let migrated = self
            .index
            .db
            .query_row(
                "SELECT value FROM state WHERE key='file-index'",
                [],
                |row| row.get::<_, bool>(0),
            )
            .optional()?
            .unwrap_or(false);
        let legacy: BTreeMap<String, u128> = if migrated {
            BTreeMap::new()
        } else {
            match read_bytes(&self.attachments, ".unused.json", 64 * 1024 * 1024) {
                Ok(bytes) => serde_json::from_slice(&bytes)
                    .map_err(|_| Error::InvalidData("The attachment cleanup index is invalid."))?,
                Err(Error::NotFound) => BTreeMap::new(),
                Err(error) => return Err(error),
            }
        };
        if legacy.keys().any(|id| !valid_attachment(id.as_bytes())) {
            return Err(Error::InvalidData(
                "The attachment cleanup index is invalid.",
            ));
        }
        let mut seen = HashSet::new();
        self.index.db.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> Result<(), Error> {
            for entry in fs::Dir::read_from(&self.attachments).map_err(io::Error::from)? {
                let entry = entry.map_err(io::Error::from)?;
                let Ok(name) = entry.file_name().to_str() else {
                    continue;
                };
                if entry.file_type() != fs::FileType::RegularFile {
                    continue;
                }
                let id = if valid_attachment(name.as_bytes()) {
                    name
                } else if let Some(id) = name
                    .strip_suffix(".json")
                    .filter(|id| valid_attachment(id.as_bytes()))
                {
                    id
                } else {
                    continue;
                };
                if !seen.insert(id.to_owned()) {
                    continue;
                }
                match self.register_file(id, now) {
                    Ok(()) => {}
                    Err(Error::InvalidFile) => continue,
                    Err(error) => return Err(error),
                }
                if let Some(since) = legacy.get(id)
                    && !self.file_referenced(id)?
                {
                    self.index
                        .db
                        .prepare_cached("UPDATE gc_files SET unused=? WHERE id=?")?
                        .execute(params![
                            i64::try_from(*since).map_err(|_| Error::InvalidData(
                                "The attachment cleanup time is invalid."
                            ))?,
                            id
                        ])?;
                }
                if seen.len() % 1000 == 0 {
                    self.index.db.execute_batch("COMMIT; BEGIN IMMEDIATE")?;
                }
                if seen.len() % 100_000 == 0 {
                    eprintln!("operation=file_index phase=reconcile files={}", seen.len());
                }
            }
            let indexed = self
                .index
                .db
                .prepare("SELECT id FROM gc_files")?
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            for id in indexed {
                if !seen.contains(&id) {
                    self.index
                        .db
                        .execute("DELETE FROM gc_files WHERE id=?", [id])?;
                }
            }
            self.index
                .db
                .execute("INSERT OR REPLACE INTO state VALUES('file-index',1)", [])?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = self.index.db.execute_batch("ROLLBACK");
            return Err(error);
        }
        self.index.db.execute_batch("COMMIT")?;
        Ok(())
    }

    pub fn collect_indexed_unused_attachments(
        &mut self,
        now: SystemTime,
        limit: usize,
    ) -> Result<usize, Error> {
        let readable: i64 = self.index.db.query_row("SELECT coalesce(sum(value),0) FROM state WHERE key IN ('notes-readable','recovery-readable')",[],|row|row.get(0))?;
        if readable != 2 {
            return Err(Error::InvalidData(
                "Attachment cleanup is paused because a source could not be read.",
            ));
        }
        if !(1..=50).contains(&limit) {
            return Err(Error::InvalidData(
                "The attachment cleanup batch is invalid.",
            ));
        }
        let now = millis(now)?;
        let dirty = self
            .index
            .db
            .prepare_cached("SELECT id FROM gc_dirty LIMIT ?")?
            .query_map([(limit * 4) as i64], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let transaction = self.index.db.unchecked_transaction()?;
        for id in dirty {
            if self.file_referenced(&id)? {
                transaction.execute("UPDATE gc_files SET unused=NULL WHERE id=?", [&id])?;
            } else {
                transaction.execute(
                    "UPDATE gc_files SET unused=coalesce(unused,?) WHERE id=?",
                    params![now, id],
                )?;
            }
            transaction.execute("DELETE FROM gc_dirty WHERE id=?", [id])?;
        }
        transaction.commit()?;
        let grace = Duration::from_secs(7 * 86400).as_millis() as i64;
        if now < grace {
            return Ok(0);
        }
        let expired=self.index.db.prepare_cached("SELECT id FROM gc_files WHERE unused IS NOT NULL AND unused<=? ORDER BY unused,id LIMIT ?")?.query_map(params![now-grace,limit as i64],|row|row.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
        let mut removed = 0;
        for id in expired {
            if !valid_attachment(id.as_bytes()) {
                return Err(Error::InvalidFile);
            }
            if self.file_referenced(&id)? {
                self.index
                    .db
                    .execute("UPDATE gc_files SET unused=NULL WHERE id=?", [id])?;
                continue;
            }
            let stamp = self.attachment_stamp(&id)?;
            let indexed: String =
                self.index
                    .db
                    .query_row("SELECT stamp FROM gc_files WHERE id=?", [&id], |row| {
                        row.get(0)
                    })?;
            if stamp != indexed {
                self.index.db.execute(
                    "UPDATE gc_files SET stamp=?,unused=? WHERE id=?",
                    params![stamp, now, id],
                )?;
                continue;
            }
            if self.encrypted {
                self.index
                    .db
                    .execute("DELETE FROM vault_files WHERE id=?", [&id])?;
            } else {
                for name in [id.clone(), format!("{id}.json")] {
                    match fs::unlinkat(&self.attachments, name, AtFlags::empty()) {
                        Ok(()) | Err(rustix::io::Errno::NOENT) => {}
                        Err(error) => return Err(Error::Io(error.into())),
                    }
                }
            }
            self.index
                .db
                .execute("DELETE FROM gc_files WHERE id=?", [id])?;
            removed += 1;
        }
        if removed > 0 {
            self.attachments.sync_all()?;
        }
        Ok(removed)
    }
}
