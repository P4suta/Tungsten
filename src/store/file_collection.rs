use super::Store;
use crate::{Error, rules::valid_attachment};
use rusqlite::{OptionalExtension, params};
use std::{
    io,
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
        Ok(self.index.db.prepare_cached("SELECT EXISTS(SELECT 1 FROM attachments WHERE target=?1 UNION ALL SELECT 1 FROM record_attachments WHERE target=?1 LIMIT 1)")?.query_row([id], |row| row.get(0))?)
    }

    fn attachment_stamp(&self, id: &str) -> Result<String, Error> {
        self.index
            .db
            .query_row("SELECT revision FROM files WHERE id=?", [id], |row| {
                row.get(0)
            })
            .optional()?
            .ok_or(Error::NotFound)
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

    pub fn collect_indexed_unused_attachments(
        &mut self,
        now: SystemTime,
        limit: usize,
    ) -> Result<usize, Error> {
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
            let transaction = self.index.db.unchecked_transaction()?;
            self.index
                .db
                .execute("DELETE FROM files WHERE id=?", [&id])?;
            self.index
                .db
                .execute("DELETE FROM gc_files WHERE id=?", [id])?;
            transaction.commit()?;
            removed += 1;
        }
        Ok(removed)
    }
}
