use super::{Engine, kv::KvStore};
use anyhow::{Result, ensure};
use std::{
    io::Read,
    ops::Range,
    path::{Path, PathBuf},
};
use tungsten::{
    domain::{NoteName, Revision},
    store::{Note, Store, WorkspaceId},
};

enum Backend {
    Sqlite(Store),
    Kv(KvStore),
}
pub(super) struct Storage {
    engine: Engine,
    path: PathBuf,
    backend: Backend,
}

impl Storage {
    pub fn load_fixture(
        &mut self,
        notes: impl Iterator<Item = Result<Note, tungsten::Error>>,
    ) -> Result<usize> {
        match &mut self.backend {
            Backend::Sqlite(store) => Ok(store.import_notes(notes)?),
            Backend::Kv(store) => store.load_fixture(notes),
        }
    }
    pub fn take_timings(&mut self) -> Option<super::kv::MutationTimings> {
        match &mut self.backend {
            Backend::Sqlite(_) => None,
            Backend::Kv(store) => store.timings.take(),
        }
    }
    pub fn open(engine: Engine, path: &Path) -> Result<Self> {
        let backend = match engine {
            Engine::Sqlite => Backend::Sqlite(Store::open(path)?),
            Engine::SqliteKv | Engine::Redb | Engine::Fjall => {
                Backend::Kv(KvStore::open(engine, path)?)
            }
            Engine::All => anyhow::bail!("a concrete engine is required"),
        };
        Ok(Self {
            engine,
            path: path.into(),
            backend,
        })
    }
    pub fn reopen(self) -> Result<Self> {
        let engine = self.engine;
        let path = self.path.clone();
        drop(self);
        Self::open(engine, &path)
    }
    pub fn read(&self, name: &str) -> Result<Note> {
        match &self.backend {
            Backend::Sqlite(store) => Ok(store.read(&NoteName::parse(name.into())?)?),
            Backend::Kv(store) => store.read(name),
        }
    }
    pub fn put(
        &mut self,
        name: &str,
        content: &str,
        expected: Option<&Revision>,
    ) -> Result<Revision> {
        match &mut self.backend {
            Backend::Sqlite(store) => Ok(match expected {
                Some(revision) => {
                    store.update(&NoteName::parse(name.into())?, content, revision.as_str())?
                }
                None => store.create(Some(name.into()), content)?,
            }
            .revision),
            Backend::Kv(store) => store.put(name, content, expected),
        }
    }
    pub fn search(&self, query: &str, titles: bool, limit: usize) -> Result<Vec<String>> {
        match &self.backend {
            Backend::Sqlite(store) => Ok(store
                .search(query, None, limit, titles)?
                .notes
                .into_iter()
                .map(|note| note.name.as_str().into())
                .collect()),
            Backend::Kv(store) => store.search(query, titles, limit),
        }
    }
    pub fn incoming(&self, name: &str) -> Result<Vec<String>> {
        match &self.backend {
            Backend::Sqlite(store) => Ok(store
                .backlinks(&NoteName::parse(name.into())?)?
                .into_iter()
                .map(|note| note.name.as_str().into())
                .collect()),
            Backend::Kv(store) => store.incoming(name),
        }
    }
    pub fn history_count(&self, name: &str) -> Result<usize> {
        match &self.backend {
            Backend::Sqlite(store) => Ok(store.history(&NoteName::parse(name.into())?)?.len()),
            Backend::Kv(store) => store.history_count(name),
        }
    }
    pub fn upload(&mut self, name: &str, length: usize, source: &mut dyn Read) -> Result<String> {
        match &mut self.backend {
            Backend::Sqlite(store) => Ok(store.upload_from(name.into(), length, source)?.id),
            Backend::Kv(store) => store.upload(name, length, source),
        }
    }
    pub fn range(&self, id: &str, range: Range<usize>) -> Result<Vec<u8>> {
        match &self.backend {
            Backend::Sqlite(store) => Ok(store.attachment_range(id, range)?),
            Backend::Kv(store) => store.range(id, range),
        }
    }
    pub fn backup(&self, output: &Path) -> Result<()> {
        match &self.backend {
            Backend::Sqlite(store) => Ok(store.backup_to(output)?),
            Backend::Kv(store) => store.backup(output, &self.path),
        }
    }
    pub fn restore(engine: Engine, input: &Path, output: &Path) -> Result<()> {
        match engine {
            Engine::Sqlite => Ok(Store::restore_from(input, output)?),
            Engine::SqliteKv | Engine::Redb | Engine::Fjall => {
                KvStore::restore(engine, input, output)
            }
            Engine::All => anyhow::bail!("a concrete engine is required"),
        }
    }
    pub fn verify_rollback(&mut self) -> Result<()> {
        match &self.backend {
            Backend::Sqlite(_) => {
                let result = rusqlite::Connection::open(self.path.join("notes.sqlite"))?;
                let tx = result.unchecked_transaction()?;
                tx.execute(
                    "INSERT INTO documents VALUES('Rollback.md','uncommitted',0)",
                    [],
                )?;
                drop(tx);
                ensure!(
                    self.read("Rollback.md").is_err(),
                    "an aborted transaction became visible"
                );
                Ok(())
            }
            Backend::Kv(store) => store.database.verify_rollback(),
        }
    }

    pub fn pin(&mut self, name: &str, revision: &Revision) -> Result<()> {
        match &mut self.backend {
            Backend::Sqlite(store) => {
                Ok(store.pin(&NoteName::parse(name.into())?, true, revision.as_str())?)
            }
            Backend::Kv(store) => store.pin(name, revision),
        }
    }
    pub fn create_workspace(&mut self, title: &str) -> Result<String> {
        match &mut self.backend {
            Backend::Sqlite(store) => Ok(store.create_workspace(title)?.id.to_string()),
            Backend::Kv(store) => store.create_workspace(title),
        }
    }
    pub fn add_member(&mut self, workspace: &str, name: &str) -> Result<()> {
        match &mut self.backend {
            Backend::Sqlite(store) => {
                store.workspace_member(
                    WorkspaceId::parse(workspace)?,
                    &NoteName::parse(name.into())?,
                    true,
                )?;
                Ok(())
            }
            Backend::Kv(store) => store.add_member(workspace, name),
        }
    }
    pub fn members(&self, workspace: &str) -> Result<Vec<String>> {
        match &self.backend {
            Backend::Sqlite(store) => Ok(store
                .workspace_notes(WorkspaceId::parse(workspace)?, None, 50)?
                .notes
                .into_iter()
                .map(|note| note.name.as_str().into())
                .collect()),
            Backend::Kv(store) => store.members(workspace),
        }
    }
    pub fn delete(&mut self, name: &str, revision: &Revision) -> Result<()> {
        match &mut self.backend {
            Backend::Sqlite(store) => {
                Ok(store.delete(&NoteName::parse(name.into())?, revision.as_str())?)
            }
            Backend::Kv(store) => store.delete(name, revision),
        }
    }
    pub fn cleanup(&mut self, timestamp: u128) -> Result<()> {
        match &mut self.backend {
            Backend::Sqlite(store) => {
                store.process_recovery_cleanup(timestamp, 50)?;
                store.collect_indexed_unused_attachments(
                    std::time::UNIX_EPOCH + std::time::Duration::from_millis(timestamp.try_into()?),
                    50,
                )?;
                Ok(())
            }
            Backend::Kv(store) => store.cleanup(timestamp),
        }
    }
}

pub(super) fn crash(engine: Engine, path: &Path, commit: bool) -> Result<()> {
    if commit {
        let mut store = Storage::open(engine, path)?;
        let revision = store.read("Crash.md")?.revision;
        store.put("Crash.md", "# Durable acknowledgement", Some(&revision))?;
        std::process::exit(73);
    }
    match engine {
        Engine::Sqlite => {
            let db = rusqlite::Connection::open(path.join("notes.sqlite"))?;
            let tx = db.unchecked_transaction()?;
            tx.execute(
                "UPDATE documents SET body='uncommitted' WHERE name='Crash.md'",
                [],
            )?;
            std::process::exit(73);
        }
        Engine::SqliteKv | Engine::Redb | Engine::Fjall => {
            let db = super::database::Database::open(engine, path)?;
            db.transaction::<()>(|write| {
                write.put(b"b/documents/Crash.md", b"uncommitted")?;
                std::process::exit(73)
            })
        }
        Engine::All => anyhow::bail!("a concrete engine is required"),
    }
}
