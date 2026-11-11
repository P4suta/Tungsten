use super::Store;
use crate::{
    Error,
    domain::{NoteName, Revision},
};
use rusqlite::{Connection, Transaction, params};
use serde::Serialize;
use std::fmt;

pub const WORKSPACE_PAGE_LIMIT: usize = 50;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceId(i64);

impl WorkspaceId {
    pub fn parse(value: &str) -> Result<Self, Error> {
        let id = value
            .parse::<i64>()
            .map_err(|_| Error::InvalidData("The workspace identifier is invalid."))?;
        if id <= 0 || id.to_string() != value {
            return Err(Error::InvalidData("The workspace identifier is invalid."));
        }
        Ok(Self(id))
    }
}

impl fmt::Display for WorkspaceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Serialize for WorkspaceId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Debug, Serialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub title: String,
    pub revision: Revision,
}

#[derive(Debug, Serialize)]
pub struct Workspaces {
    pub workspaces: Vec<Workspace>,
    pub next: Option<String>,
    pub revision: Revision,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceEntry {
    pub name: NoteName,
    pub title: String,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceNotes {
    pub workspace: Workspace,
    pub notes: Vec<WorkspaceEntry>,
    pub next: Option<String>,
    pub revision: Revision,
}

fn title(value: &str) -> Result<String, Error> {
    let control = value
        .chars()
        .any(|character| character.is_control() || matches!(character, '\u{2028}' | '\u{2029}'));
    let value = value.trim();
    if value.is_empty() || value.len() > 480 || value.chars().count() > 120 || control {
        return Err(Error::InvalidData(
            "Use a workspace name of 1 to 120 characters without line breaks.",
        ));
    }
    Ok(value.to_owned())
}

fn page(after: Option<&str>, limit: usize) -> Result<i64, Error> {
    if !(1..=WORKSPACE_PAGE_LIMIT).contains(&limit) {
        return Err(Error::InvalidData("The workspace page limit is invalid."));
    }
    after.map_or(Ok(0), |value| WorkspaceId::parse(value).map(|id| id.0))
}

pub(super) fn create_schema(transaction: &Transaction<'_>) -> Result<(), Error> {
    transaction.execute_batch("CREATE TABLE workspaces(id INTEGER PRIMARY KEY AUTOINCREMENT,title TEXT NOT NULL UNIQUE,revision INTEGER NOT NULL DEFAULT 0 CHECK(typeof(revision)='integer' AND revision>=0));
        CREATE TABLE workspace_state(id INTEGER PRIMARY KEY CHECK(id=1),revision INTEGER NOT NULL CHECK(typeof(revision)='integer' AND revision>=0));
        INSERT INTO workspace_state VALUES(1,0);
        CREATE TABLE workspace_members(seq INTEGER PRIMARY KEY AUTOINCREMENT,workspace INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,note TEXT NOT NULL REFERENCES documents(name) ON DELETE CASCADE ON UPDATE CASCADE,UNIQUE(workspace,note));
        CREATE INDEX workspace_note_order ON workspace_members(workspace,seq);
        CREATE INDEX workspace_note_sources ON workspace_members(note);
        CREATE TRIGGER workspace_insert AFTER INSERT ON workspaces BEGIN UPDATE workspace_state SET revision=revision+1 WHERE id=1; END;
        CREATE TRIGGER workspace_update AFTER UPDATE ON workspaces BEGIN UPDATE workspace_state SET revision=revision+1 WHERE id=1; END;
        CREATE TRIGGER workspace_delete AFTER DELETE ON workspaces BEGIN UPDATE workspace_state SET revision=revision+1 WHERE id=1; END;
        CREATE TRIGGER workspace_member_insert AFTER INSERT ON workspace_members BEGIN UPDATE workspaces SET revision=revision+1 WHERE id=new.workspace; END;
        CREATE TRIGGER workspace_member_delete AFTER DELETE ON workspace_members BEGIN UPDATE workspaces SET revision=revision+1 WHERE id=old.workspace; END;
        CREATE TRIGGER workspace_member_update AFTER UPDATE ON workspace_members BEGIN UPDATE workspaces SET revision=revision+1 WHERE id IN(old.workspace,new.workspace); END;
        CREATE TRIGGER workspace_note_title AFTER UPDATE OF summary ON notes WHEN json_extract(old.summary,'$.title')<>json_extract(new.summary,'$.title') BEGIN UPDATE workspaces SET revision=revision+1 WHERE id IN(SELECT workspace FROM workspace_members WHERE note=new.name); END;")?;
    Ok(())
}

pub(super) fn check_schema(db: &Connection) -> Result<(), Error> {
    for (kind, names) in [
        (
            "table",
            &["workspaces", "workspace_members", "workspace_state"][..],
        ),
        (
            "index",
            &["workspace_note_order", "workspace_note_sources"][..],
        ),
        (
            "trigger",
            &[
                "workspace_insert",
                "workspace_update",
                "workspace_delete",
                "workspace_member_insert",
                "workspace_member_delete",
                "workspace_member_update",
                "workspace_note_title",
            ][..],
        ),
    ] {
        for name in names {
            if !db.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type=? AND name=?)",
                params![kind, name],
                |row| row.get::<_, bool>(0),
            )? {
                return Err(Error::InvalidData("The workspace schema is incomplete."));
            }
        }
    }
    db.prepare("SELECT id,title,revision FROM workspaces LIMIT 0")?;
    db.prepare("SELECT seq,workspace,note FROM workspace_members LIMIT 0")?;
    let revision: i64 = db.query_row(
        "SELECT revision FROM workspace_state WHERE id=1",
        [],
        |row| row.get(0),
    )?;
    if revision < 0 {
        return Err(Error::InvalidFile);
    }
    Ok(())
}

fn workspace(row: &rusqlite::Row<'_>) -> Result<Workspace, Error> {
    let id: i64 = row.get(0)?;
    let name: String = row.get(1)?;
    let counter: i64 = row.get(2)?;
    if id <= 0 || counter < 0 || title(&name)? != name {
        return Err(Error::InvalidFile);
    }
    Ok(Workspace {
        id: WorkspaceId(id),
        revision: Revision::of_parts(&[
            b"workspace",
            &id.to_be_bytes(),
            &counter.to_be_bytes(),
            name.as_bytes(),
        ]),
        title: name,
    })
}

impl Store {
    pub fn workspace(&self, id: WorkspaceId) -> Result<Workspace, Error> {
        let mut statement = self
            .index
            .db
            .prepare_cached("SELECT id,title,revision FROM workspaces WHERE id=?")?;
        let mut rows = statement.query([id.0])?;
        workspace(rows.next()?.ok_or(Error::WorkspaceMissing)?)
    }

    pub fn workspaces(&self, after: Option<&str>, limit: usize) -> Result<Workspaces, Error> {
        let after = page(after, limit)?;
        let counter: i64 = self.index.db.query_row(
            "SELECT revision FROM workspace_state WHERE id=1",
            [],
            |row| row.get(0),
        )?;
        let mut statement = self.index.db.prepare_cached(
            "SELECT id,title,revision FROM workspaces WHERE id>? ORDER BY id LIMIT ?",
        )?;
        let mut rows = statement.query(params![after, limit as i64 + 1])?;
        let mut workspaces = Vec::new();
        while let Some(row) = rows.next()? {
            workspaces.push(workspace(row)?);
        }
        let more = workspaces.len() > limit;
        workspaces.truncate(limit);
        let next = if more {
            workspaces.last().map(|entry| entry.id.to_string())
        } else {
            None
        };
        Ok(Workspaces {
            workspaces,
            next,
            revision: Revision::of_parts(&[
                b"workspaces",
                &counter.to_be_bytes(),
                &after.to_be_bytes(),
                &(limit as u64).to_be_bytes(),
            ]),
        })
    }

    pub fn create_workspace(&mut self, name: &str) -> Result<Workspace, Error> {
        let name = title(name)?;
        let transaction = self.index.db.unchecked_transaction()?;
        if transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM workspaces WHERE title=?)",
            [&name],
            |row| row.get::<_, bool>(0),
        )? {
            return Err(Error::WorkspaceExists);
        }
        transaction.execute("INSERT INTO workspaces(title) VALUES(?)", [&name])?;
        let id = WorkspaceId(transaction.last_insert_rowid());
        let created = self.workspace(id)?;
        transaction.commit()?;
        Ok(created)
    }

    pub fn rename_workspace(
        &mut self,
        id: WorkspaceId,
        name: &str,
        expected: &str,
    ) -> Result<Workspace, Error> {
        let name = title(name)?;
        let transaction = self.index.db.unchecked_transaction()?;
        let current = self.workspace(id)?;
        if !current.revision.matches(expected) {
            return Err(Error::WorkspaceChanged);
        }
        if transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM workspaces WHERE title=? AND id<>?)",
            params![name, id.0],
            |row| row.get::<_, bool>(0),
        )? {
            return Err(Error::WorkspaceExists);
        }
        if current.title != name {
            transaction.execute(
                "UPDATE workspaces SET title=?,revision=revision+1 WHERE id=?",
                params![name, id.0],
            )?;
        }
        let updated = self.workspace(id)?;
        transaction.commit()?;
        Ok(updated)
    }

    pub fn delete_workspace(&mut self, id: WorkspaceId, expected: &str) -> Result<(), Error> {
        let transaction = self.index.db.unchecked_transaction()?;
        if !self.workspace(id)?.revision.matches(expected) {
            return Err(Error::WorkspaceChanged);
        }
        transaction.execute("DELETE FROM workspaces WHERE id=?", [id.0])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn workspace_member(
        &mut self,
        id: WorkspaceId,
        note: &NoteName,
        included: bool,
    ) -> Result<Workspace, Error> {
        let transaction = self.index.db.unchecked_transaction()?;
        self.workspace(id)?;
        if included {
            if self.index.summary(note.as_str())?.is_none() {
                return Err(Error::NotFound);
            }
            transaction.execute("INSERT INTO workspace_members(workspace,note) VALUES(?,?) ON CONFLICT(workspace,note) DO NOTHING", params![id.0,note.as_str()])?;
        } else {
            transaction.execute(
                "DELETE FROM workspace_members WHERE workspace=? AND note=?",
                params![id.0, note.as_str()],
            )?;
        }
        let updated = self.workspace(id)?;
        transaction.commit()?;
        Ok(updated)
    }

    pub fn workspace_notes(
        &self,
        id: WorkspaceId,
        after: Option<&str>,
        limit: usize,
    ) -> Result<WorkspaceNotes, Error> {
        let after = page(after, limit)?;
        let workspace = self.workspace(id)?;
        let mut statement = self.index.db.prepare_cached("SELECT m.seq,m.note,json_extract(n.summary,'$.title') FROM workspace_members m JOIN notes n ON n.name=m.note WHERE m.workspace=? AND m.seq>? ORDER BY m.seq LIMIT ?")?;
        let mut rows = statement.query(params![id.0, after, limit as i64 + 1])?;
        let mut notes = Vec::new();
        let mut last = None;
        while let Some(row) = rows.next()? {
            let sequence: i64 = row.get(0)?;
            if notes.len() < limit {
                last = Some(sequence.to_string());
            }
            notes.push(WorkspaceEntry {
                name: NoteName::parse(row.get(1)?)?,
                title: row.get(2)?,
            });
        }
        let more = notes.len() > limit;
        notes.truncate(limit);
        Ok(WorkspaceNotes {
            revision: Revision::of_parts(&[
                workspace.revision.as_str().as_bytes(),
                &after.to_be_bytes(),
                &(limit as u64).to_be_bytes(),
            ]),
            workspace,
            notes,
            next: if more { last } else { None },
        })
    }
}
