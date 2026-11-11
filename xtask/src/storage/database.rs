use super::Engine;
use anyhow::{Result, ensure};
use fjall::Readable;
use redb::{ReadableDatabase, TableDefinition};
use std::{collections::BTreeMap, path::Path};

const TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("records");

pub(super) trait Write {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()>;
    fn remove(&mut self, key: &[u8]) -> Result<()>;
    fn adjust_count(&mut self, key: &[u8], delta: i64) -> Result<()> {
        match adjusted_count(self.get(key)?.as_deref(), delta)? {
            Some(value) => self.put(key, &value),
            None => self.remove(key),
        }
    }
}

fn adjusted_count(value: Option<&[u8]>, delta: i64) -> Result<Option<Vec<u8>>> {
    let count = value.map_or(Ok(0), |bytes| {
        let count = u64::from_be_bytes(bytes.try_into()?);
        ensure!(count > 0, "invalid stored posting count");
        Ok::<_, anyhow::Error>(count)
    })?;
    let updated = count
        .checked_add_signed(delta)
        .ok_or_else(|| anyhow::anyhow!("invalid posting count transition"))?;
    Ok((updated != 0).then(|| updated.to_be_bytes().to_vec()))
}

struct BatchWrite<'a> {
    inner: &'a mut dyn Write,
    counts: BTreeMap<Vec<u8>, i64>,
}

impl Write for BatchWrite<'_> {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let value = self.inner.get(key)?;
        match self.counts.get(key) {
            Some(delta) => adjusted_count(value.as_deref(), *delta),
            None => Ok(value),
        }
    }
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.counts.remove(key);
        self.inner.put(key, value)
    }
    fn remove(&mut self, key: &[u8]) -> Result<()> {
        self.counts.remove(key);
        self.inner.remove(key)
    }
    fn adjust_count(&mut self, key: &[u8], delta: i64) -> Result<()> {
        let current = self.counts.entry(key.to_vec()).or_default();
        *current = current
            .checked_add(delta)
            .ok_or_else(|| anyhow::anyhow!("posting count delta overflow"))?;
        Ok(())
    }
}

fn batch<R>(
    write: &mut dyn Write,
    operation: impl FnOnce(&mut dyn Write) -> Result<R>,
) -> Result<R> {
    let mut grouped = BatchWrite {
        inner: write,
        counts: BTreeMap::new(),
    };
    let result = operation(&mut grouped)?;
    for (key, delta) in grouped.counts {
        if delta != 0 {
            grouped.inner.adjust_count(&key, delta)?;
        }
    }
    Ok(result)
}

impl Write for redb::Table<'_, &[u8], &[u8]> {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        Ok(redb::ReadableTable::get(self, key)?.map(|value| value.value().to_vec()))
    }
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.insert(key, value)?;
        Ok(())
    }
    fn remove(&mut self, key: &[u8]) -> Result<()> {
        self.remove(key)?;
        Ok(())
    }
}

struct FjallWrite<'a, 'b> {
    transaction: &'a mut fjall::SingleWriterWriteTx<'b>,
    keyspace: &'a fjall::SingleWriterTxKeyspace,
}

struct SqliteWrite<'a, 'b>(&'a rusqlite::Transaction<'b>);

impl Write for SqliteWrite<'_, '_> {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        use rusqlite::OptionalExtension;
        Ok(self
            .0
            .prepare_cached("SELECT value FROM records WHERE key=?")?
            .query_row([key], |row| row.get(0))
            .optional()?)
    }
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.0.prepare_cached("INSERT INTO records(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value")?.execute(rusqlite::params![key, value])?;
        Ok(())
    }
    fn remove(&mut self, key: &[u8]) -> Result<()> {
        self.0
            .prepare_cached("DELETE FROM records WHERE key=?")?
            .execute([key])?;
        Ok(())
    }
}

impl Write for FjallWrite<'_, '_> {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        Ok(self
            .transaction
            .get(self.keyspace, key)?
            .map(|value| value.to_vec()))
    }
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.transaction.insert(self.keyspace, key, value);
        Ok(())
    }
    fn remove(&mut self, key: &[u8]) -> Result<()> {
        self.transaction.remove(self.keyspace, key);
        Ok(())
    }
}

pub(super) enum Database {
    Sqlite {
        writer: rusqlite::Connection,
        reader: rusqlite::Connection,
    },
    Redb(redb::Database),
    Fjall {
        database: fjall::SingleWriterTxDatabase,
        keyspace: fjall::SingleWriterTxKeyspace,
    },
}

impl Database {
    pub fn open(engine: Engine, path: &Path) -> Result<Self> {
        std::fs::create_dir_all(path)?;
        match engine {
            Engine::SqliteKv => {
                let file = path.join("records.sqlite");
                let writer = rusqlite::Connection::open(&file)?;
                writer.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA cache_size=-2048; PRAGMA mmap_size=0; PRAGMA temp_store=FILE; CREATE TABLE IF NOT EXISTS records(key BLOB PRIMARY KEY,value BLOB NOT NULL) WITHOUT ROWID;")?;
                let reader = rusqlite::Connection::open(&file)?;
                reader.execute_batch("PRAGMA query_only=ON; PRAGMA cache_size=-64; PRAGMA mmap_size=0; PRAGMA temp_store=FILE;")?;
                Ok(Self::Sqlite { writer, reader })
            }
            Engine::Redb => {
                let db = redb::Database::builder()
                    .set_cache_size(2 * 1024 * 1024)
                    .create(path.join("records.redb"))?;
                let tx = db.begin_write()?;
                {
                    tx.open_table(TABLE)?;
                }
                tx.commit()?;
                Ok(Self::Redb(db))
            }
            Engine::Fjall => {
                let database = fjall::SingleWriterTxDatabase::builder(path.join("records.fjall"))
                    .cache_size(2 * 1024 * 1024)
                    .open()?;
                let keyspace = database.keyspace("records", || {
                    fjall::KeyspaceCreateOptions::default().max_memtable_size(4 * 1024 * 1024)
                })?;
                Ok(Self::Fjall { database, keyspace })
            }
            Engine::Sqlite | Engine::All => {
                anyhow::bail!("a concrete key-value engine is required")
            }
        }
    }

    pub fn transaction<R>(&self, operation: impl FnOnce(&mut dyn Write) -> Result<R>) -> Result<R> {
        match self {
            Self::Sqlite { writer, .. } => {
                let tx = writer.unchecked_transaction()?;
                let result = batch(&mut SqliteWrite(&tx), operation)?;
                tx.commit()?;
                Ok(result)
            }
            Self::Redb(database) => {
                let mut tx = database.begin_write()?;
                tx.set_durability(redb::Durability::Immediate)?;
                let result = {
                    let mut table = tx.open_table(TABLE)?;
                    batch(&mut table, operation)?
                };
                tx.commit()?;
                Ok(result)
            }
            Self::Fjall { database, keyspace } => {
                let mut tx = database
                    .write_tx()
                    .durability(Some(fjall::PersistMode::SyncAll));
                let result = batch(
                    &mut FjallWrite {
                        transaction: &mut tx,
                        keyspace,
                    },
                    operation,
                )?;
                tx.commit()?;
                Ok(result)
            }
        }
    }

    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        match self {
            Self::Sqlite { reader, .. } => {
                use rusqlite::OptionalExtension;
                Ok(reader
                    .prepare_cached("SELECT value FROM records WHERE key=?")?
                    .query_row([key], |row| row.get(0))
                    .optional()?)
            }
            Self::Redb(database) => Ok(database
                .begin_read()?
                .open_table(TABLE)?
                .get(key)?
                .map(|value| value.value().to_vec())),
            Self::Fjall { keyspace, .. } => Ok(keyspace.get(key)?.map(|value| value.to_vec())),
        }
    }

    pub fn scan(
        &self,
        prefix: &[u8],
        mut visit: impl FnMut(&[u8], &[u8]) -> Result<bool>,
    ) -> Result<()> {
        match self {
            Self::Sqlite { reader, .. } => {
                let mut upper = prefix.to_vec();
                while upper.last() == Some(&u8::MAX) {
                    upper.pop();
                }
                let mut statement = if let Some(last) = upper.last_mut() {
                    *last += 1;
                    reader.prepare_cached(
                        "SELECT key,value FROM records WHERE key>=?1 AND key<?2 ORDER BY key",
                    )?
                } else {
                    reader.prepare_cached(
                        "SELECT key,value FROM records WHERE key>=?1 ORDER BY key",
                    )?
                };
                let mut rows = if upper.is_empty() {
                    statement.query([prefix])?
                } else {
                    statement.query(rusqlite::params![prefix, upper])?
                };
                while let Some(row) = rows.next()? {
                    let key: Vec<u8> = row.get(0)?;
                    let value: Vec<u8> = row.get(1)?;
                    if !visit(&key, &value)? {
                        break;
                    }
                }
            }
            Self::Redb(database) => {
                let tx = database.begin_read()?;
                let table = tx.open_table(TABLE)?;
                for row in table.range(prefix..)? {
                    let (key, value) = row?;
                    if !key.value().starts_with(prefix) || !visit(key.value(), value.value())? {
                        break;
                    }
                }
            }
            Self::Fjall { database, keyspace } => {
                for row in database.read_tx().prefix(keyspace, prefix) {
                    let (key, value) = row.into_inner()?;
                    if !visit(&key, &value)? {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    pub fn verify_rollback(&self) -> Result<()> {
        let result = self.transaction::<()>(|write| {
            write.put(b"rollback-probe", b"uncommitted")?;
            anyhow::bail!("injected abort")
        });
        ensure!(
            result.is_err() && self.get(b"rollback-probe")?.is_none(),
            "an aborted transaction became visible"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_posting_count_transitions_never_wrap_or_coerce() {
        assert!(adjusted_count(None, -1).is_err());
        assert!(adjusted_count(Some(&u64::MAX.to_be_bytes()), 1).is_err());
        assert!(adjusted_count(Some(&[0; 8]), 1).is_err());
        assert!(adjusted_count(Some(&[1]), 1).is_err());
        assert_eq!(adjusted_count(Some(&1u64.to_be_bytes()), -1).unwrap(), None);
        assert_eq!(
            adjusted_count(None, 2).unwrap(),
            Some(2u64.to_be_bytes().to_vec())
        );
    }
}
