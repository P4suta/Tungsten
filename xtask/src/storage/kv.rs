use super::{
    Engine,
    database::{Database, Write},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read},
    ops::Range,
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tungsten::{
    backup::{self, Cell, Kind, Reader, Record, StagedDirectory, Writer},
    domain::{NoteName, Revision},
    store::{MAX_ATTACHMENT_BYTES, MAX_NOTE_BYTES, Note, recovery_checkpoint_due},
};

const CHUNK: usize = 64 * 1024;
const INDEX_FORMAT: &[u8] = b"counted-packed-postings";
mod index;
mod lifecycle;

fn text(value: impl Into<String>) -> Cell {
    Cell::Text(value.into())
}
fn integer(value: u128) -> Result<Cell> {
    Ok(Cell::Integer(value.try_into()?))
}
fn source(kind: Kind, id: &str) -> String {
    format!("s/{}/{id}", kind.table())
}
fn body(kind: Kind, id: &str) -> String {
    format!("b/{}/{id}", kind.table())
}
fn now() -> Result<u128> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())
}
fn hash(digest: Sha256) -> Result<Revision> {
    Ok(Revision::from_digest(digest))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Meta {
    name: String,
    id: u128,
    modified: u128,
    revision: Revision,
    title: String,
    tags: Vec<String>,
    links: Vec<String>,
    files: Vec<String>,
    pinned: bool,
    open_task: bool,
}

fn metadata(note: &Note, id: u128, pinned: bool) -> Meta {
    let extracted = tungsten::markdown::index_metadata(note.name.as_str(), &note.body);
    let title = extracted.title.unwrap_or_else(|| {
        note.body
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("Untitled note")
            .trim()
            .trim_start_matches('#')
            .trim()
            .to_owned()
    });
    Meta {
        name: note.name.as_str().into(),
        id,
        modified: note.modified,
        revision: note.revision.clone(),
        title,
        tags: extracted.tags,
        links: extracted.links,
        files: tungsten::markdown::attachment_references(&note.body),
        pinned,
        open_task: note.body.contains("[ ]"),
    }
}

fn update_index(write: &mut dyn Write, meta: &Meta, content: &str, insert: bool) -> Result<()> {
    index::Plan::between(
        (!insert).then_some((meta, content)),
        insert.then_some((meta, content)),
    )
    .apply(write, &meta.name)
}

fn put_record(write: &mut dyn Write, id: &str, record: &Record) -> Result<()> {
    record.validate()?;
    write.put(
        source(record.kind, id).as_bytes(),
        &serde_json::to_vec(record)?,
    )
}

pub(super) struct KvStore {
    pub database: Database,
    pub timings: Option<MutationTimings>,
}

pub(super) struct MutationTimings {
    pub preparation_us: u64,
    pub transaction_us: u64,
    pub index_changes: usize,
}

impl KvStore {
    pub fn load_fixture(
        &mut self,
        mut notes: impl Iterator<Item = Result<Note, tungsten::Error>>,
    ) -> Result<usize> {
        ensure!(
            self.counter(Kind::State, "count")? == 0,
            "fixture loading requires an empty store"
        );
        let mut count = 0usize;
        loop {
            let mut batch = Vec::new();
            let mut bytes = 0;
            for note in notes.by_ref().take(500) {
                let note = note?;
                bytes += note.body.len();
                batch.push(note);
                if bytes >= 4 * 1024 * 1024 {
                    break;
                }
            }
            if batch.is_empty() {
                break;
            }
            self.database.transaction(|write| {
                for note in &batch {
                    ensure!(
                        note.body.len() <= MAX_NOTE_BYTES
                            && note.revision == Revision::of(note.body.as_bytes()),
                        "invalid fixture note"
                    );
                    count += 1;
                    let meta = metadata(note, count as u128, false);
                    put_record(
                        write,
                        note.name.as_str(),
                        &Record {
                            kind: Kind::Document,
                            values: vec![
                                text(note.name.as_str()),
                                text(note.revision.to_string()),
                                integer(note.modified)?,
                                integer(count as u128)?,
                            ],
                        },
                    )?;
                    write.put(
                        body(Kind::Document, note.name.as_str()).as_bytes(),
                        note.body.as_bytes(),
                    )?;
                    write.put(
                        format!("m/{}", note.name.as_str()).as_bytes(),
                        &serde_json::to_vec(&meta)?,
                    )?;
                    update_index(write, &meta, &note.body, true)?;
                }
                for (kind, name) in [(Kind::State, "count"), (Kind::Sequence, "notes")] {
                    put_record(
                        write,
                        name,
                        &Record {
                            kind,
                            values: vec![text(name), integer(count as u128)?],
                        },
                    )?;
                }
                Ok(())
            })?;
        }
        Ok(count)
    }

    pub fn open(engine: Engine, path: &Path) -> Result<Self> {
        let database = Database::open(engine, path)?;
        let store = Self {
            database,
            timings: None,
        };
        if store
            .database
            .get(source(Kind::State, "count").as_bytes())?
            .is_none()
        {
            store.database.transaction(|write| {
                write.put(b"\0profile-index", INDEX_FORMAT)?;
                for name in ["count", "record-sequence", "trash-purge"] {
                    put_record(
                        write,
                        name,
                        &Record {
                            kind: Kind::State,
                            values: vec![text(name), Cell::Integer(0)],
                        },
                    )?;
                }
                put_record(
                    write,
                    "1",
                    &Record {
                        kind: Kind::WorkspaceState,
                        values: vec![Cell::Integer(1), Cell::Integer(0)],
                    },
                )
            })?;
        } else {
            ensure!(
                store.database.get(b"\0profile-index")?.as_deref() == Some(INDEX_FORMAT),
                "unsupported prototype index; restore a portable archive into a fresh directory"
            );
        }
        Ok(store)
    }

    fn record(&self, kind: Kind, id: &str) -> Result<Record> {
        let record: Record = serde_json::from_slice(
            &self
                .database
                .get(source(kind, id).as_bytes())?
                .context("missing source record")?,
        )?;
        record.validate()?;
        Ok(record)
    }

    fn counter(&self, kind: Kind, name: &str) -> Result<u128> {
        match self.database.get(source(kind, name).as_bytes())? {
            None => Ok(0),
            Some(value) => Ok(serde_json::from_slice::<Record>(&value)?.integer(1)? as u128),
        }
    }

    pub fn read(&self, name: &str) -> Result<Note> {
        let record = self.record(Kind::Document, name)?;
        let content = self
            .database
            .get(body(Kind::Document, name).as_bytes())?
            .context("missing Markdown")?;
        let revision = Revision::of(&content);
        ensure!(
            revision.matches(record.text(1)?),
            "Markdown checksum mismatch"
        );
        Ok(Note {
            name: NoteName::parse(name.into())?,
            body: String::from_utf8(content)?,
            modified: record.integer(2)? as u128,
            revision,
        })
    }

    fn meta(&self, name: &str) -> Result<Meta> {
        Ok(serde_json::from_slice(
            &self
                .database
                .get(format!("m/{name}").as_bytes())?
                .context("missing note index")?,
        )?)
    }

    fn history(&self, name: &str) -> Result<Vec<Record>> {
        let mut records = Vec::new();
        self.database
            .scan(format!("h/{name}\0").as_bytes(), |_, value| {
                let record = self.record(Kind::Snapshot, std::str::from_utf8(value)?)?;
                records.push(record);
                if records.len() > 50 {
                    records.remove(0);
                }
                Ok(true)
            })?;
        Ok(records)
    }

    pub fn put(
        &mut self,
        name: &str,
        content: &str,
        expected: Option<&Revision>,
    ) -> Result<Revision> {
        let started = Instant::now();
        self.timings = None;
        ensure!(content.len() <= MAX_NOTE_BYTES, "note limit exceeded");
        let name = NoteName::parse(name.into())?;
        let old = self
            .database
            .get(source(Kind::Document, name.as_str()).as_bytes())?
            .map(|_| self.read(name.as_str()))
            .transpose()?;
        match (&old, expected) {
            (None, None) => {}
            (Some(note), Some(expected)) if &note.revision == expected => {}
            _ => return Err(tungsten::Error::Conflict.into()),
        }
        if let Some(old) = &old
            && old.body == content
        {
            return Ok(old.revision.clone());
        }
        let timestamp = now()?;
        let count = self.counter(Kind::State, "count")?;
        let id = if old.is_some() {
            self.record(Kind::Document, name.as_str())?.integer(3)? as u128
        } else {
            self.counter(Kind::Sequence, "notes")?
                .checked_add(1)
                .context("note sequence overflow")?
        };
        let note = Note {
            name,
            body: content.into(),
            modified: timestamp,
            revision: Revision::of(content.as_bytes()),
        };
        let previous_meta = old
            .as_ref()
            .map(|note| self.meta(note.name.as_str()))
            .transpose()?;
        let current_meta = metadata(
            &note,
            id,
            previous_meta.as_ref().is_some_and(|meta| meta.pinned),
        );
        let records = self.history(note.name.as_str())?;
        let latest = records
            .last()
            .map(|record| {
                let bytes = self
                    .database
                    .get(body(Kind::Snapshot, record.text(1)?).as_bytes())?
                    .context("missing checkpoint")?;
                Ok::<_, anyhow::Error>((
                    Note {
                        name: NoteName::parse(record.text(2)?.into())?,
                        body: String::from_utf8(bytes)?,
                        revision: Revision::parse(record.text(3)?.into())?,
                        modified: record.integer(4)? as u128,
                    },
                    record.integer(5)? as u128,
                ))
            })
            .transpose()?;
        let snapshot = old.as_ref().filter(|old| {
            recovery_checkpoint_due(
                old,
                latest.as_ref().map(|(note, captured)| (note, *captured)),
                timestamp,
            )
        });
        let sequence = self.counter(Kind::State, "record-sequence")?;
        let plan = index::Plan::between(
            old.as_ref()
                .zip(previous_meta.as_ref())
                .map(|(note, meta)| (meta, note.body.as_str())),
            Some((&current_meta, content)),
        );
        let preparation_us = started.elapsed().as_micros().try_into()?;
        let transaction_started = Instant::now();
        self.database.transaction(|write| {
            if let Some(old) = snapshot {
                let sequence = sequence
                    .checked_add(1)
                    .context("recovery sequence overflow")?;
                let saved_id = format!("{sequence:032x}");
                let record = Record {
                    kind: Kind::Snapshot,
                    values: vec![
                        Cell::Integer(0),
                        text(&saved_id),
                        text(old.name.as_str()),
                        text(old.revision.to_string()),
                        integer(old.modified)?,
                        integer(timestamp)?,
                        integer(sequence)?,
                    ],
                };
                put_record(write, &saved_id, &record)?;
                write.put(
                    body(Kind::Snapshot, &saved_id).as_bytes(),
                    old.body.as_bytes(),
                )?;
                write.put(
                    format!("h/{}\0{saved_id}", old.name.as_str()).as_bytes(),
                    saved_id.as_bytes(),
                )?;
                for id in tungsten::markdown::attachment_references(&old.body) {
                    write.put(
                        format!("r/{id}\0{saved_id}").as_bytes(),
                        saved_id.as_bytes(),
                    )?;
                }
                put_record(
                    write,
                    "record-sequence",
                    &Record {
                        kind: Kind::State,
                        values: vec![text("record-sequence"), integer(sequence)?],
                    },
                )?;
                for expired in records.iter().take(records.len().saturating_sub(49)) {
                    let id = expired.text(1)?;
                    write.remove(source(Kind::Snapshot, id).as_bytes())?;
                    write.remove(body(Kind::Snapshot, id).as_bytes())?;
                    write.remove(format!("h/{}\0{id}", old.name.as_str()).as_bytes())?;
                    if let Some(bytes) = self.database.get(body(Kind::Snapshot, id).as_bytes())? {
                        for file in
                            tungsten::markdown::attachment_references(std::str::from_utf8(&bytes)?)
                        {
                            write.remove(format!("r/{file}\0{id}").as_bytes())?;
                        }
                    }
                }
            }
            put_record(
                write,
                note.name.as_str(),
                &Record {
                    kind: Kind::Document,
                    values: vec![
                        text(note.name.as_str()),
                        text(note.revision.to_string()),
                        integer(timestamp)?,
                        integer(id)?,
                    ],
                },
            )?;
            write.put(
                body(Kind::Document, note.name.as_str()).as_bytes(),
                content.as_bytes(),
            )?;
            write.put(
                format!("m/{}", note.name.as_str()).as_bytes(),
                &serde_json::to_vec(&current_meta)?,
            )?;
            plan.apply(write, note.name.as_str())?;
            for id in &current_meta.files {
                if let Some(bytes) = self.database.get(source(Kind::FileClock, id).as_bytes())? {
                    let mut clock: Record = serde_json::from_slice(&bytes)?;
                    *clock.values.get_mut(2).context("missing reference clock")? = Cell::Null;
                    put_record(write, id, &clock)?;
                }
            }
            if old.is_none() {
                put_record(
                    write,
                    "notes",
                    &Record {
                        kind: Kind::Sequence,
                        values: vec![text("notes"), integer(id)?],
                    },
                )?;
                put_record(
                    write,
                    "count",
                    &Record {
                        kind: Kind::State,
                        values: vec![text("count"), integer(count + 1)?],
                    },
                )?;
            }
            Ok(())
        })?;
        self.timings = Some(MutationTimings {
            preparation_us,
            transaction_us: transaction_started.elapsed().as_micros().try_into()?,
            index_changes: plan.changes(),
        });
        Ok(note.revision)
    }

    pub fn search(&self, query: &str, titles: bool, limit: usize) -> Result<Vec<String>> {
        ensure!(
            (1..=200).contains(&limit)
                && query.len() <= 512
                && query.split_whitespace().count() <= 32,
            "invalid search"
        );
        let query = query.trim().to_lowercase();
        let terms: Vec<_> = query.split_whitespace().collect();
        if terms.contains(&"-path:") {
            return Ok(Vec::new());
        }
        let indexed = terms.iter().any(|term| {
            let term = term.strip_prefix('-').unwrap_or(term);
            !term.is_empty()
                && !term.starts_with('#')
                && !term.starts_with("tag:")
                && !term.starts_with("path:")
                && !matches!(term, "is:pinned" | "task:open")
        });
        let prioritize = !terms.is_empty()
            && terms.iter().all(|term| {
                !term.starts_with(['-', '#'])
                    && !term.starts_with("tag:")
                    && !term.starts_with("path:")
                    && !matches!(*term, "is:pinned" | "task:open")
            });
        let deadline = Instant::now() + Duration::from_millis(750);
        let mut requirements = Vec::new();
        let prefix = if indexed {
            let scope = if titles { b't' } else { b'g' };
            let mut best = vec![b'n'];
            let mut smallest = u64::MAX;
            for term in terms.iter().filter(|term| !term.starts_with('-')) {
                let candidates = if let Some(path) = term.strip_prefix("path:") {
                    if path.is_empty() {
                        Vec::new()
                    } else {
                        vec![index::path(path)]
                    }
                } else if let Some(tag) =
                    term.strip_prefix('#').or_else(|| term.strip_prefix("tag:"))
                {
                    vec![index::filter(b'@', tag)]
                } else if *term == "is:pinned" {
                    vec![vec![b'P']]
                } else if *term == "task:open" {
                    vec![vec![b'K']]
                } else {
                    index::candidates(scope, term)
                };
                for candidate in candidates {
                    ensure!(Instant::now() < deadline, "search deadline exceeded");
                    let count = self.posting_count(&candidate)?;
                    requirements.push((count, candidate.clone()));
                    if count < smallest {
                        smallest = count;
                        best = candidate;
                    }
                    if count == 0 {
                        break;
                    }
                }
                if smallest == 0 {
                    break;
                }
            }
            best
        } else {
            terms
                .iter()
                .find_map(|term| {
                    if let Some(tag) = term.strip_prefix('#').or_else(|| term.strip_prefix("tag:"))
                    {
                        Some(index::filter(b'#', tag))
                    } else if *term == "is:pinned" {
                        Some(vec![b'p'])
                    } else if *term == "task:open" {
                        Some(vec![b'k'])
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| vec![b'o'])
        };
        requirements.sort_unstable();
        requirements.dedup();
        let intersect =
            requirements.len() > 1 && requirements.first().is_some_and(|(count, _)| *count > 1);
        let total: u64 = if intersect {
            self.counter(Kind::State, "count")?.try_into()?
        } else {
            0
        };
        let mut prefixes = Vec::new();
        if prioritize {
            prefixes.push((true, index::exact(&query)));
        }
        prefixes.push((false, prefix));
        let mut matches = Vec::new();
        for (exact, prefix) in prefixes {
            if matches.len() == limit {
                break;
            }
            self.database.scan(&prefix, |key, value| {
                ensure!(Instant::now() < deadline, "search deadline exceeded");
                if !exact && intersect {
                    ensure!(key.len() == prefix.len() + 16, "invalid posting key");
                    let suffix = &key[prefix.len()..];
                    for (count, required) in &requirements {
                        if required == &prefix || *count == total {
                            continue;
                        }
                        ensure!(Instant::now() < deadline, "search deadline exceeded");
                        let candidate = [required.as_slice(), suffix].concat();
                        if self.database.get(&candidate)?.as_deref() != Some(value) {
                            return Ok(true);
                        }
                    }
                }
                let name = std::str::from_utf8(value)?;
                if matches.iter().any(|item| item == name) {
                    return Ok(true);
                }
                let meta = self.meta(name)?;
                if exact && meta.title.to_lowercase() != query {
                    return Ok(true);
                }
                if exact {
                    matches.push(meta.name);
                    return Ok(matches.len() < limit);
                }
                let content = if !indexed {
                    String::new()
                } else if titles {
                    meta.title.to_lowercase()
                } else {
                    format!("{}\n{}", meta.name, self.read(&meta.name)?.body).to_lowercase()
                };
                let matched = terms.iter().all(|term| {
                    let (term, negate) = term
                        .strip_prefix('-')
                        .map_or((*term, false), |term| (term, true));
                    if term.is_empty() {
                        return true;
                    }
                    let value = if let Some(tag) =
                        term.strip_prefix('#').or_else(|| term.strip_prefix("tag:"))
                    {
                        meta.tags.iter().any(|value| value == tag)
                    } else if let Some(path) = term.strip_prefix("path:") {
                        meta.name.to_ascii_lowercase().starts_with(path)
                    } else if term == "is:pinned" {
                        meta.pinned
                    } else if term == "task:open" {
                        meta.open_task
                    } else {
                        content.contains(term)
                    };
                    value != negate
                });
                if matched {
                    matches.push(meta.name);
                }
                Ok(matches.len() < limit)
            })?;
        }
        Ok(matches)
    }

    fn posting_count(&self, prefix: &[u8]) -> Result<u64> {
        if let Some(bytes) = self.database.get(&index::count_key(prefix))? {
            let count = u64::from_be_bytes(bytes.as_slice().try_into()?);
            ensure!(count > 0, "invalid posting count");
            Ok(count)
        } else {
            let mut present = false;
            self.database.scan(prefix, |_, _| {
                present = true;
                Ok(false)
            })?;
            ensure!(!present, "missing posting count");
            Ok(0)
        }
    }

    pub fn incoming(&self, name: &str) -> Result<Vec<String>> {
        let mut result = Vec::new();
        self.database
            .scan(format!("l/{name}\0").as_bytes(), |_, value| {
                result.push(std::str::from_utf8(value)?.to_owned());
                Ok(result.len() < 120)
            })?;
        Ok(result)
    }

    pub fn history_count(&self, name: &str) -> Result<usize> {
        Ok(self.history(name)?.len())
    }

    pub fn upload(&mut self, name: &str, length: usize, input: &mut dyn Read) -> Result<String> {
        ensure!(length <= MAX_ATTACHMENT_BYTES, "attachment limit exceeded");
        tungsten::store::AttachmentMeta::Name(name.into()).validate()?;
        let id = format!("{}.bin", uuid::Uuid::new_v4().simple());
        let meta = serde_json::to_vec(name)?;
        let timestamp = now()?;
        self.database.transaction(|write| {
            let mut digest = Sha256::new();
            let mut buffer = [0; CHUNK];
            let mut head = Vec::new();
            for start in (0..length).step_by(CHUNK) {
                let count = (length - start).min(CHUNK);
                input.read_exact(&mut buffer[..count])?;
                if start == 0 {
                    head.extend_from_slice(&buffer[..count.min(512)]);
                }
                digest.update(&buffer[..count]);
                write.put(
                    format!("f/{id}/{:08}", start / CHUNK).as_bytes(),
                    &buffer[..count],
                )?;
            }
            ensure!(
                input.read(&mut buffer[..1])? == 0,
                "attachment length changed"
            );
            let data_revision = hash(digest.clone())?;
            digest.update(&meta);
            let revision = hash(digest)?;
            put_record(
                write,
                &id,
                &Record {
                    kind: Kind::File,
                    values: vec![
                        text(&id),
                        Cell::Bytes(meta.clone()),
                        Cell::Bytes(head),
                        text(revision.to_string()),
                        text(data_revision.to_string()),
                        integer(timestamp)?,
                        integer(length as u128)?,
                    ],
                },
            )?;
            put_record(
                write,
                &id,
                &Record {
                    kind: Kind::FileClock,
                    values: vec![text(&id), text(revision.to_string()), integer(timestamp)?],
                },
            )?;
            Ok(())
        })?;
        Ok(id)
    }

    pub fn range(&self, id: &str, range: Range<usize>) -> Result<Vec<u8>> {
        let length = self.record(Kind::File, id)?.integer(6)? as usize;
        ensure!(
            range.start <= range.end && range.end <= length,
            "invalid file range"
        );
        let mut output = Vec::with_capacity(range.len());
        let mut offset = range.start;
        while offset < range.end {
            let chunk = self
                .database
                .get(format!("f/{id}/{:08}", offset / CHUNK).as_bytes())?
                .context("missing file chunk")?;
            let start = offset % CHUNK;
            let count = (range.end - offset).min(
                chunk
                    .len()
                    .checked_sub(start)
                    .context("invalid file chunk")?,
            );
            ensure!(count > 0, "empty file chunk");
            output.extend_from_slice(&chunk[start..start + count]);
            offset += count;
        }
        Ok(output)
    }

    pub fn backup(&self, output: &Path, path: &Path) -> Result<()> {
        let staging = StagedDirectory::new(output, path)?;
        let mut writer = Writer::new(staging.path())?;
        for kind in Kind::ALL {
            self.database
                .scan(format!("s/{}/", kind.table()).as_bytes(), |_, value| {
                    let record: Record = serde_json::from_slice(value)?;
                    match kind {
                        Kind::Document | Kind::Snapshot => {
                            let id = record.text(if kind == Kind::Document { 0 } else { 1 })?;
                            let bytes = self
                                .database
                                .get(body(kind, id).as_bytes())?
                                .context("missing archive payload")?;
                            writer.record(&record, Some(&mut bytes.as_slice()))?;
                        }
                        Kind::File => {
                            let mut payload = BlobReader {
                                store: self,
                                id: record.text(0)?.into(),
                                offset: 0,
                                length: record.integer(6)? as usize,
                            };
                            writer.record(&record, Some(&mut payload))?;
                        }
                        _ => writer.record(&record, None)?,
                    }
                    Ok(true)
                })?;
        }
        writer.finish()?;
        staging.publish()?;
        Ok(())
    }

    pub fn restore(engine: Engine, input: &Path, output: &Path) -> Result<()> {
        let staging = StagedDirectory::new(output, input)?;
        let database = Database::open(engine, staging.path())?;
        let mut reader = Reader::open(input)?;
        while let Some(record) = reader.next_record()? {
            let id = match record.kind {
                Kind::Snapshot => record.text(1)?.to_owned(),
                Kind::TitleUpdate | Kind::Workspace | Kind::Membership | Kind::WorkspaceState => {
                    record.integer(0)?.to_string()
                }
                _ => record.text(0)?.to_owned(),
            };
            ensure!(
                database.get(source(record.kind, &id).as_bytes())?.is_none(),
                "duplicate source record"
            );
            database.transaction(|write| {
                match record.kind {
                    Kind::Document | Kind::Snapshot => {
                        let mut bytes = Vec::new();
                        backup::copy_payload(&record, &mut reader.payload(&record)?, &mut bytes)?;
                        let content = String::from_utf8(bytes)?;
                        write.put(body(record.kind, &id).as_bytes(), content.as_bytes())?;
                        if record.kind == Kind::Document {
                            let note = Note {
                                name: NoteName::parse(id.clone())?,
                                revision: Revision::parse(record.text(1)?.into())?,
                                modified: record.integer(2)? as u128,
                                body: content,
                            };
                            let meta = metadata(&note, record.integer(3)? as u128, false);
                            write.put(format!("m/{id}").as_bytes(), &serde_json::to_vec(&meta)?)?;
                            update_index(write, &meta, &note.body, true)?;
                        } else if record.integer(0)? == 0 {
                            write.put(
                                format!("h/{}\0{id}", record.text(2)?).as_bytes(),
                                id.as_bytes(),
                            )?;
                        }
                    }
                    Kind::File => {
                        let mut payload = reader.payload(&record)?;
                        let mut sink = ChunkWriter {
                            write,
                            id: &id,
                            offset: 0,
                            buffer: Vec::new(),
                        };
                        backup::copy_payload(&record, &mut payload, &mut sink)?;
                        io::Write::flush(&mut sink)?;
                    }
                    _ => {}
                }
                put_record(write, &id, &record)
            })?;
        }
        ensure!(
            database
                .get(source(Kind::State, "count").as_bytes())?
                .is_some(),
            "missing restored state"
        );
        let store = Self {
            database,
            timings: None,
        };
        store.finish_restore()?;
        store
            .database
            .transaction(|write| write.put(b"\0profile-index", INDEX_FORMAT))?;
        drop(store);
        staging.publish()?;
        Ok(())
    }
}

struct BlobReader<'a> {
    store: &'a KvStore,
    id: String,
    offset: usize,
    length: usize,
}
impl Read for BlobReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let count = output.len().min(self.length - self.offset).min(CHUNK);
        if count == 0 {
            return Ok(0);
        }
        let data = self
            .store
            .range(&self.id, self.offset..self.offset + count)
            .map_err(io::Error::other)?;
        output[..count].copy_from_slice(&data);
        self.offset += count;
        Ok(count)
    }
}

struct ChunkWriter<'a> {
    write: &'a mut dyn Write,
    id: &'a str,
    offset: usize,
    buffer: Vec<u8>,
}
impl io::Write for ChunkWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let count = bytes.len().min(CHUNK - self.buffer.len());
        self.buffer.extend_from_slice(&bytes[..count]);
        if self.buffer.len() == CHUNK {
            self.flush()?;
        }
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        if !self.buffer.is_empty() {
            self.write
                .put(
                    format!("f/{}/{:08}", self.id, self.offset / CHUNK).as_bytes(),
                    &self.buffer,
                )
                .map_err(io::Error::other)?;
            self.offset += self.buffer.len();
            self.buffer.clear();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_loading_bounds_transaction_batches_by_notes_and_bytes() {
        for (size, count, boundary) in [(1024, 501, 500), (1024 * 1024, 5, 4)] {
            let directory = tempfile::tempdir().unwrap();
            let mut store = KvStore::open(Engine::SqliteKv, directory.path()).unwrap();
            let reader =
                rusqlite::Connection::open(directory.path().join("records.sqlite")).unwrap();
            let mut observations = Vec::new();
            store
                .load_fixture((0..count).map(|id| {
                    if id == boundary - 1 || id == boundary {
                        let bytes: Vec<u8> = reader
                            .query_row(
                                "SELECT value FROM records WHERE key=?",
                                [source(Kind::State, "count").as_bytes()],
                                |row| row.get(0),
                            )
                            .unwrap();
                        let record: Record = serde_json::from_slice(&bytes).unwrap();
                        observations.push(record.integer(1).unwrap());
                    }
                    let body = "a".repeat(size);
                    Ok(Note {
                        name: NoteName::parse(format!("note-{id:04}.md"))?,
                        revision: Revision::of(body.as_bytes()),
                        body,
                        modified: 42,
                    })
                }))
                .unwrap();
            assert_eq!(observations, [0, i64::from(boundary)], "size={size}");
        }
    }

    #[test]
    fn identifier_search_uses_counts_beyond_the_former_sample_limit() {
        for engine in [Engine::SqliteKv, Engine::Redb, Engine::Fjall] {
            let directory = tempfile::tempdir().unwrap();
            let mut store = KvStore::open(engine, directory.path()).unwrap();
            store
                .load_fixture((0..257).map(|id| {
                    let body = if id == 0 {
                        "# Topic\n\nneedle-00099999".to_owned()
                    } else if id == 1 {
                        "# Topic\n\nneedle-000 999".to_owned()
                    } else {
                        format!(
                            "# Topic\n\nneedle-000 009 099 {}",
                            if id < 129 { "999" } else { "" }
                        )
                    };
                    Ok(Note {
                        name: NoteName::parse(format!("note-{id:04}.md"))?,
                        revision: Revision::of(body.as_bytes()),
                        body,
                        modified: 42,
                    })
                }))
                .unwrap();
            let rare = index::prefix(b'g', "999");
            assert_eq!(store.posting_count(&rare).unwrap(), 129);
            assert_eq!(
                store.posting_count(&index::prefix(b'g', "nee")).unwrap(),
                257
            );
            store
                .database
                .transaction(|write| {
                    write.put(b"m/note-0256.md", b"invalid unrelated metadata")?;
                    write.put(b"m/note-0001.md", b"invalid nonmatching metadata")
                })
                .unwrap();
            assert_eq!(
                store.search("needle-00099999", false, 120).unwrap(),
                ["note-0000.md"],
                "{engine:?}"
            );
            assert_eq!(
                store.search("path:note-0000 -absent", false, 120).unwrap(),
                ["note-0000.md"],
                "{engine:?}"
            );
            for invalid in [None, Some(vec![0; 8]), Some(vec![1])] {
                store
                    .database
                    .transaction(|write| match &invalid {
                        Some(value) => write.put(&index::count_key(&rare), value),
                        None => write.remove(&index::count_key(&rare)),
                    })
                    .unwrap();
                assert!(
                    store.search("needle-00099999", false, 120).is_err(),
                    "{engine:?}"
                );
            }
        }
    }
}
