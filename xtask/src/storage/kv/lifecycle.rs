use super::{
    Cell, Kind, KvStore, Record, body, integer, metadata, now, put_record, source, text,
    update_index,
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeSet;
use tungsten::domain::Revision;

impl KvStore {
    pub fn pin(&mut self, name: &str, expected: &Revision) -> Result<()> {
        let note = self.read(name)?;
        ensure!(&note.revision == expected, "stale pin revision");
        let previous = self.meta(name)?;
        let mut meta = previous.clone();
        meta.pinned = true;
        let plan =
            super::index::Plan::between(Some((&previous, &note.body)), Some((&meta, &note.body)));
        self.database.transaction(|write| {
            plan.apply(write, name)?;
            put_record(
                write,
                name,
                &Record {
                    kind: Kind::Pin,
                    values: vec![text(name)],
                },
            )?;
            write.put(format!("m/{name}").as_bytes(), &serde_json::to_vec(&meta)?)
        })
    }

    pub fn create_workspace(&mut self, title: &str) -> Result<String> {
        let id = self
            .counter(Kind::Sequence, "workspaces")?
            .checked_add(1)
            .context("workspace sequence overflow")?;
        let revision = self
            .record(Kind::WorkspaceState, "1")?
            .integer(1)?
            .checked_add(1)
            .context("workspace revision overflow")?;
        let record = Record {
            kind: Kind::Workspace,
            values: vec![integer(id)?, text(title), Cell::Integer(0)],
        };
        record.validate()?;
        self.database.transaction(|write| {
            put_record(write, &id.to_string(), &record)?;
            put_record(
                write,
                "workspaces",
                &Record {
                    kind: Kind::Sequence,
                    values: vec![text("workspaces"), integer(id)?],
                },
            )?;
            put_record(
                write,
                "1",
                &Record {
                    kind: Kind::WorkspaceState,
                    values: vec![Cell::Integer(1), Cell::Integer(revision)],
                },
            )
        })?;
        Ok(id.to_string())
    }

    pub fn add_member(&mut self, workspace: &str, name: &str) -> Result<()> {
        self.read(name)?;
        let lookup = format!("wm/{workspace}\0{name}");
        if self.database.get(lookup.as_bytes())?.is_some() {
            return Ok(());
        }
        let mut group = self.record(Kind::Workspace, workspace)?;
        let revision = group
            .integer(2)?
            .checked_add(1)
            .context("workspace revision overflow")?;
        *group
            .values
            .get_mut(2)
            .context("missing workspace revision")? = Cell::Integer(revision);
        let global = self
            .record(Kind::WorkspaceState, "1")?
            .integer(1)?
            .checked_add(1)
            .context("workspace revision overflow")?;
        let seq = self
            .counter(Kind::Sequence, "workspace_members")?
            .checked_add(1)
            .context("membership sequence overflow")?;
        self.database.transaction(|write| {
            put_record(
                write,
                &seq.to_string(),
                &Record {
                    kind: Kind::Membership,
                    values: vec![integer(seq)?, Cell::Integer(group.integer(0)?), text(name)],
                },
            )?;
            put_record(write, workspace, &group)?;
            put_record(
                write,
                "1",
                &Record {
                    kind: Kind::WorkspaceState,
                    values: vec![Cell::Integer(1), Cell::Integer(global)],
                },
            )?;
            put_record(
                write,
                "workspace_members",
                &Record {
                    kind: Kind::Sequence,
                    values: vec![text("workspace_members"), integer(seq)?],
                },
            )?;
            write.put(lookup.as_bytes(), seq.to_string().as_bytes())?;
            write.put(
                format!("w/{workspace}/{seq:020}").as_bytes(),
                name.as_bytes(),
            )
        })
    }

    pub fn members(&self, workspace: &str) -> Result<Vec<String>> {
        self.record(Kind::Workspace, workspace)?;
        let mut names = Vec::new();
        self.database
            .scan(format!("w/{workspace}/").as_bytes(), |_, value| {
                names.push(std::str::from_utf8(value)?.into());
                Ok(names.len() < 50)
            })?;
        Ok(names)
    }

    pub fn delete(&mut self, name: &str, expected: &Revision) -> Result<()> {
        let old = self.read(name)?;
        ensure!(&old.revision == expected, "stale deletion revision");
        let meta = self.meta(name)?;
        let sequence = self
            .counter(Kind::State, "record-sequence")?
            .checked_add(1)
            .context("recovery sequence overflow")?;
        let count = self
            .counter(Kind::State, "count")?
            .checked_sub(1)
            .context("note count underflow")?;
        let id = format!("{sequence:032x}");
        let mut memberships = Vec::new();
        self.database.scan(
            format!("s/{}/", Kind::Membership.table()).as_bytes(),
            |_, value| {
                let record: Record = serde_json::from_slice(value)?;
                if record.text(2)? == name {
                    memberships.push(record);
                }
                Ok(true)
            },
        )?;
        let global = self
            .record(Kind::WorkspaceState, "1")?
            .integer(1)?
            .checked_add(memberships.len().try_into()?)
            .context("workspace revision overflow")?;
        let timestamp = now()?;
        self.database.transaction(|write| {
            put_record(
                write,
                &id,
                &Record {
                    kind: Kind::Snapshot,
                    values: vec![
                        Cell::Integer(1),
                        text(&id),
                        text(name),
                        text(old.revision.to_string()),
                        integer(old.modified)?,
                        integer(timestamp)?,
                        integer(sequence)?,
                    ],
                },
            )?;
            write.put(body(Kind::Snapshot, &id).as_bytes(), old.body.as_bytes())?;
            for file in &meta.files {
                write.put(format!("r/{file}\0{id}").as_bytes(), id.as_bytes())?;
            }
            update_index(write, &meta, &old.body, false)?;
            write.remove(source(Kind::Document, name).as_bytes())?;
            write.remove(source(Kind::Pin, name).as_bytes())?;
            write.remove(body(Kind::Document, name).as_bytes())?;
            write.remove(format!("m/{name}").as_bytes())?;
            put_record(
                write,
                "record-sequence",
                &Record {
                    kind: Kind::State,
                    values: vec![text("record-sequence"), integer(sequence)?],
                },
            )?;
            put_record(
                write,
                "count",
                &Record {
                    kind: Kind::State,
                    values: vec![text("count"), integer(count)?],
                },
            )?;
            for member in memberships {
                let workspace = member.integer(1)?.to_string();
                let seq = member.integer(0)?;
                write.remove(source(Kind::Membership, &seq.to_string()).as_bytes())?;
                write.remove(format!("wm/{workspace}\0{name}").as_bytes())?;
                write.remove(format!("w/{workspace}/{seq:020}").as_bytes())?;
                let mut group = self.record(Kind::Workspace, &workspace)?;
                let revision = group
                    .integer(2)?
                    .checked_add(1)
                    .context("workspace revision overflow")?;
                *group
                    .values
                    .get_mut(2)
                    .context("missing workspace revision")? = Cell::Integer(revision);
                put_record(write, &workspace, &group)?;
            }
            put_record(
                write,
                "1",
                &Record {
                    kind: Kind::WorkspaceState,
                    values: vec![Cell::Integer(1), Cell::Integer(global)],
                },
            )?;
            Ok(())
        })
    }

    pub fn cleanup(&mut self, timestamp: u128) -> Result<()> {
        let mut expired = Vec::new();
        self.database.scan(
            format!("s/{}/", Kind::Snapshot.table()).as_bytes(),
            |_, value| {
                let record: Record = serde_json::from_slice(value)?;
                if timestamp.saturating_sub(record.integer(5)? as u128) >= 7 * 24 * 60 * 60 * 1000 {
                    expired.push(record);
                }
                Ok(expired.len() < 50)
            },
        )?;
        self.database.transaction(|write| {
            for record in &expired {
                let id = record.text(1)?;
                let bytes = self
                    .database
                    .get(body(Kind::Snapshot, id).as_bytes())?
                    .context("missing recovery body")?;
                for file in tungsten::markdown::attachment_references(std::str::from_utf8(&bytes)?)
                {
                    write.remove(format!("r/{file}\0{id}").as_bytes())?;
                }
                write.remove(format!("h/{}\0{id}", record.text(2)?).as_bytes())?;
                write.remove(source(Kind::Snapshot, id).as_bytes())?;
                write.remove(body(Kind::Snapshot, id).as_bytes())?;
            }
            Ok(())
        })?;
        let mut clocks = Vec::new();
        self.database.scan(
            format!("s/{}/", Kind::FileClock.table()).as_bytes(),
            |_, value| {
                clocks.push(serde_json::from_slice::<Record>(value)?);
                Ok(clocks.len() < 50)
            },
        )?;
        for mut clock in clocks {
            let id = clock.text(0)?.to_owned();
            let mut referenced = false;
            for prefix in ["a", "r"] {
                self.database
                    .scan(format!("{prefix}/{id}\0").as_bytes(), |_, _| {
                        referenced = true;
                        Ok(false)
                    })?;
            }
            let unused = match clock.values.get(2) {
                Some(Cell::Integer(value)) => Some(*value as u128),
                _ => None,
            };
            if referenced || unused.is_none() {
                *clock.values.get_mut(2).context("missing cleanup clock")? = if referenced {
                    Cell::Null
                } else {
                    integer(timestamp)?
                };
                self.database
                    .transaction(|write| put_record(write, &id, &clock))?;
            } else if unused
                .is_some_and(|unused| timestamp.saturating_sub(unused) >= 7 * 24 * 60 * 60 * 1000)
            {
                let length = self.record(Kind::File, &id)?.integer(6)? as usize;
                self.database.transaction(|write| {
                    for offset in (0..length).step_by(super::CHUNK) {
                        write.remove(format!("f/{id}/{:08}", offset / super::CHUNK).as_bytes())?;
                    }
                    write.remove(source(Kind::File, &id).as_bytes())?;
                    write.remove(source(Kind::FileClock, &id).as_bytes())
                })?;
            }
        }
        Ok(())
    }

    pub(super) fn finish_restore(&self) -> Result<()> {
        let mut documents = 0u128;
        self.database.scan(
            format!("s/{}/", Kind::Document.table()).as_bytes(),
            |_, value| {
                let record: Record = serde_json::from_slice(value)?;
                let note = self.read(record.text(0)?)?;
                let pinned = self
                    .database
                    .get(source(Kind::Pin, note.name.as_str()).as_bytes())?
                    .is_some();
                let meta = metadata(&note, record.integer(3)? as u128, pinned);
                let previous = self.meta(note.name.as_str())?;
                let plan = super::index::Plan::between(
                    Some((&previous, &note.body)),
                    Some((&meta, &note.body)),
                );
                self.database.transaction(|write| {
                    plan.apply(write, note.name.as_str())?;
                    write.put(
                        format!("m/{}", note.name.as_str()).as_bytes(),
                        &serde_json::to_vec(&meta)?,
                    )
                })?;
                documents += 1;
                Ok(true)
            },
        )?;
        ensure!(
            documents == self.counter(Kind::State, "count")?,
            "restored count differs"
        );
        let mut sequences = BTreeSet::new();
        self.database.scan(
            format!("s/{}/", Kind::Snapshot.table()).as_bytes(),
            |_, value| {
                let record: Record = serde_json::from_slice(value)?;
                ensure!(
                    sequences.insert(record.integer(6)?),
                    "duplicate recovery sequence"
                );
                ensure!(
                    record.integer(6)? as u128 <= self.counter(Kind::State, "record-sequence")?,
                    "invalid recovery high-water mark"
                );
                let bytes = self
                    .database
                    .get(body(Kind::Snapshot, record.text(1)?).as_bytes())?
                    .context("missing checkpoint")?;
                self.database.transaction(|write| {
                    for file in
                        tungsten::markdown::attachment_references(std::str::from_utf8(&bytes)?)
                    {
                        write.put(
                            format!("r/{file}\0{}", record.text(1)?).as_bytes(),
                            record.text(1)?.as_bytes(),
                        )?;
                    }
                    Ok(())
                })?;
                Ok(true)
            },
        )?;
        self.database.scan(
            format!("s/{}/", Kind::Membership.table()).as_bytes(),
            |_, value| {
                let record: Record = serde_json::from_slice(value)?;
                let workspace = record.integer(1)?.to_string();
                let seq = record.integer(0)?;
                self.record(Kind::Workspace, &workspace)?;
                self.read(record.text(2)?)?;
                self.database.transaction(|write| {
                    write.put(
                        format!("wm/{workspace}\0{}", record.text(2)?).as_bytes(),
                        seq.to_string().as_bytes(),
                    )?;
                    write.put(
                        format!("w/{workspace}/{seq:020}").as_bytes(),
                        record.text(2)?.as_bytes(),
                    )
                })?;
                Ok(true)
            },
        )?;
        self.record(Kind::WorkspaceState, "1")?;
        Ok(())
    }
}
