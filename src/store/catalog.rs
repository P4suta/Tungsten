use super::{Note, Summary};
use crate::{Error, domain::NoteName};
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params, params_from_iter, types::Value};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashSet},
    fmt::Write,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

const DATABASE_FORMAT: i64 = 3;
const APPLICATION_ID: i64 = 0x54534c54;

#[derive(Serialize)]
pub struct Page {
    pub notes: Vec<Summary>,
    pub next: Option<String>,
    pub total: usize,
}

#[derive(Serialize)]
pub struct Edge {
    pub source: String,
    pub target: String,
}

#[derive(Serialize)]
pub struct Graph {
    pub notes: Vec<Summary>,
    pub edges: Vec<Edge>,
    pub total: usize,
    pub neighbors: usize,
    pub truncated: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    query: String,
    titles: bool,
    id: i64,
    pinned: bool,
    modified: i64,
    name: String,
    exact: bool,
}

pub(super) struct Catalog {
    pub db: Connection,
    clock: Instant,
    deadline: Arc<AtomicU64>,
}

fn data_error(_: serde_json::Error) -> Error {
    Error::InvalidData("The catalog data is invalid.")
}

fn encoded(chars: &[char]) -> String {
    format!("g{:x}", gram_key(chars))
}

fn gram_key(chars: &[char]) -> u64 {
    chars
        .iter()
        .fold(0, |key, ch| (key << 21) | (*ch as u64 + 1))
}

fn grams(text: &str, trigrams: bool) -> String {
    let mut tokens = HashSet::new();
    let mut window = ['\0'; 3];
    let mut length = 0;
    for character in text.chars() {
        window.rotate_left(1);
        window[2] = character;
        length = (length + 1).min(3);
        for width in 1..=length.min(if trigrams { 3 } else { 2 }) {
            tokens.insert(gram_key(&window[3 - width..]));
        }
    }
    let mut tokens = tokens.into_iter().collect::<Vec<_>>();
    tokens.sort_unstable();
    let mut result = String::with_capacity(tokens.len() * 18 + 4);
    for token in tokens {
        let _ = write!(result, "g{token:x} ");
    }
    result.push_str("all");
    result
}

enum SearchField {
    Body,
    Title,
}

fn write_search_text(
    db: &Connection,
    id: i64,
    field: SearchField,
    text: &str,
) -> Result<(), Error> {
    let (positional, distinct) = match field {
        SearchField::Body => ("fulltext", "shorttext"),
        SearchField::Title => ("titletext", "shorttitle"),
    };
    let compact = text.len() > 16 * 1024 || text.contains('\0');
    let mut tokens = grams(text, compact);
    tokens.push_str(if compact { " set" } else { " pos" });
    db.prepare_cached(&format!(
        "INSERT OR REPLACE INTO {positional}(rowid,text,k) VALUES(?,?,?)"
    ))?
    .execute(params![
        id,
        if compact { "" } else { text },
        if compact { "all set" } else { "all" }
    ])?;
    db.prepare_cached(&format!(
        "INSERT OR REPLACE INTO {distinct}(rowid,grams) VALUES(?,?)"
    ))?
    .execute(params![id, tokens])?;
    Ok(())
}

fn fts_expression(positive: &[String], negative: &[String], universe: &str) -> String {
    let base = if positive.is_empty() {
        universe.to_owned()
    } else {
        positive.join(" AND ")
    };
    if negative.is_empty() {
        base
    } else {
        format!("({base}) NOT ({})", negative.join(" OR "))
    }
}

fn trigram_expression(term: &str) -> String {
    let characters: Vec<_> = term.chars().collect();
    if characters.len() <= 3 {
        encoded(&characters)
    } else {
        characters
            .windows(3)
            .map(encoded)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(" AND ")
    }
}

fn decode_cursor(query: &str, cursor: Option<&str>, titles: bool) -> Result<Option<Cursor>, Error> {
    let cursor = cursor
        .map(|text| {
            if text.len() > 8192 {
                return Err(Error::InvalidData("The search cursor is invalid."));
            }
            let bytes = BASE64_URL_SAFE_NO_PAD
                .decode(text)
                .map_err(|_| Error::InvalidData("The search cursor is invalid."))?;
            let cursor: Cursor = serde_json::from_slice(&bytes).map_err(data_error)?;
            if cursor.query != query || cursor.titles != titles || cursor.id < 0 {
                return Err(Error::InvalidData(
                    "The search cursor belongs to another query.",
                ));
            }
            Ok(cursor)
        })
        .transpose()?;
    Ok(cursor)
}

fn encode_cursor(cursor: Cursor) -> Result<String, Error> {
    serde_json::to_vec(&cursor)
        .map(|bytes| BASE64_URL_SAFE_NO_PAD.encode(bytes))
        .map_err(data_error)
}

impl Catalog {
    pub fn open(path: &Path) -> Result<Self, Error> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let path = path
            .parent()
            .ok_or(Error::InvalidFile)?
            .canonicalize()?
            .join(path.file_name().ok_or(Error::InvalidFile)?);
        let existed = path.try_exists()?;
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let candidate = path.with_file_name(format!(
                "{}{suffix}",
                path.file_name()
                    .ok_or(Error::InvalidFile)?
                    .to_string_lossy()
            ));
            match std::fs::symlink_metadata(candidate) {
                Ok(meta)
                    if !meta.is_file() || meta.nlink() != 1 || (!existed && !suffix.is_empty()) =>
                {
                    return Err(Error::InvalidFile);
                }
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(Error::Io(error));
                }
                _ => {}
            }
        }
        let db = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        db.busy_timeout(Duration::from_secs(2))?;
        let format: i64 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let identity: i64 = db.pragma_query_value(None, "application_id", |row| row.get(0))?;
        let initialize = !existed
            && format == 0
            && identity == 0
            && db.query_row("SELECT count(*) FROM sqlite_schema", [], |row| {
                row.get::<_, i64>(0)
            })? == 0;
        if !initialize && (format != DATABASE_FORMAT || identity != APPLICATION_ID) {
            return Err(Error::InvalidData(
                "The notes database format is unsupported.",
            ));
        }
        if initialize {
            db.pragma_update(None, "auto_vacuum", 2)?;
        }
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA cache_size=-2048; PRAGMA temp_store=FILE; PRAGMA foreign_keys=ON; PRAGMA mmap_size=0; PRAGMA wal_autocheckpoint=1000; PRAGMA journal_size_limit=8388608;")?;
        if initialize {
            let transaction = db.unchecked_transaction()?;
            transaction.execute_batch("CREATE TABLE notes (
            id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE, summary TEXT NOT NULL,
            stamp TEXT NOT NULL, title_search TEXT NOT NULL,
            modified INTEGER NOT NULL, pinned INTEGER NOT NULL, open_task INTEGER NOT NULL,
            tags TEXT NOT NULL, degree INTEGER NOT NULL DEFAULT 0, outgoing INTEGER NOT NULL DEFAULT 0);
          CREATE INDEX recent_order ON notes(pinned DESC, modified DESC, name DESC);
          CREATE INDEX titles ON notes(title_search);
          CREATE INDEX hubs ON notes(degree DESC, name);
          CREATE TABLE links(source INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE, target TEXT NOT NULL, PRIMARY KEY(source,target)) WITHOUT ROWID;
          CREATE INDEX incoming ON links(target,source);
          CREATE TABLE attachments(source INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE, target TEXT NOT NULL, PRIMARY KEY(source,target)) WITHOUT ROWID;
          CREATE INDEX attachment_sources ON attachments(target,source);
          CREATE TABLE records(trash INTEGER NOT NULL,id TEXT NOT NULL,name TEXT NOT NULL,captured INTEGER NOT NULL,summary TEXT NOT NULL,stamp TEXT NOT NULL,seq INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(trash,id)) WITHOUT ROWID;
          CREATE INDEX record_name ON records(trash,name,captured DESC,id);
          CREATE INDEX record_recent ON records(trash,captured DESC,id);
          CREATE INDEX record_expiry ON records(captured,id);
          CREATE INDEX record_sequence ON records(seq);
          CREATE INDEX record_kind_sequence ON records(trash,seq);
          CREATE UNIQUE INDEX record_unique_sequence ON records(seq);
          CREATE TABLE record_attachments(trash INTEGER NOT NULL,record TEXT NOT NULL,target TEXT NOT NULL,PRIMARY KEY(trash,record,target),FOREIGN KEY(trash,record) REFERENCES records(trash,id) ON DELETE CASCADE) WITHOUT ROWID;
          CREATE INDEX recovery_references ON record_attachments(target);
          CREATE TABLE retention_queue(name TEXT PRIMARY KEY) WITHOUT ROWID;
          CREATE TABLE gc_files(id TEXT PRIMARY KEY,stamp TEXT NOT NULL,unused INTEGER) WITHOUT ROWID;
          CREATE INDEX gc_expiry ON gc_files(unused,id);
          CREATE TABLE gc_dirty(id TEXT PRIMARY KEY) WITHOUT ROWID;
          CREATE TABLE trash_visible(id TEXT PRIMARY KEY,seq INTEGER NOT NULL,captured INTEGER NOT NULL,summary TEXT NOT NULL) WITHOUT ROWID;
          CREATE INDEX trash_recent ON trash_visible(captured DESC,id);
          CREATE VIRTUAL TABLE fulltext USING fts5(text,k,tokenize='trigram case_sensitive 1',content='',contentless_delete=1);
          CREATE VIRTUAL TABLE titletext USING fts5(text,k,tokenize='trigram case_sensitive 1',content='',contentless_delete=1);
          CREATE VIRTUAL TABLE shorttext USING fts5(grams,content='',contentless_delete=1,detail=none);
          CREATE VIRTUAL TABLE shorttitle USING fts5(grams,content='',contentless_delete=1,detail=none);
          CREATE TABLE retitles(id INTEGER PRIMARY KEY AUTOINCREMENT, target TEXT NOT NULL, previous TEXT NOT NULL, title TEXT NOT NULL, revision TEXT NOT NULL, after_id INTEGER NOT NULL DEFAULT 0, ready INTEGER NOT NULL DEFAULT 0);
          CREATE TABLE state(key TEXT PRIMARY KEY, value INTEGER NOT NULL) WITHOUT ROWID;
          INSERT INTO state VALUES('count',0),('record-sequence',0),('trash-purge',0);
          CREATE TRIGGER count_insert AFTER INSERT ON notes BEGIN UPDATE state SET value=value+1 WHERE key='count'; END;
          CREATE TRIGGER count_delete AFTER DELETE ON notes BEGIN UPDATE state SET value=value-1 WHERE key='count'; END;
          CREATE TABLE documents(name TEXT PRIMARY KEY,body TEXT NOT NULL,modified INTEGER NOT NULL) WITHOUT ROWID;
          CREATE TABLE pins(name TEXT PRIMARY KEY) WITHOUT ROWID;
          CREATE TABLE snapshots(trash INTEGER NOT NULL,id TEXT NOT NULL,name TEXT NOT NULL,body TEXT NOT NULL,revision TEXT NOT NULL,modified INTEGER NOT NULL,captured INTEGER NOT NULL,PRIMARY KEY(trash,id)) WITHOUT ROWID;
          CREATE TABLE files(id TEXT PRIMARY KEY,meta BLOB NOT NULL,head BLOB NOT NULL,revision TEXT NOT NULL,body_revision TEXT NOT NULL,modified INTEGER NOT NULL,bytes INTEGER NOT NULL) WITHOUT ROWID;
          CREATE TABLE file_data(id TEXT NOT NULL UNIQUE REFERENCES files(id) ON DELETE CASCADE,data BLOB NOT NULL);")?;
            super::workspaces::create_schema(&transaction)?;
            transaction.pragma_update(None, "application_id", APPLICATION_ID)?;
            transaction.pragma_update(None, "user_version", DATABASE_FORMAT)?;
            transaction.commit()?;
        } else {
            for table in [
                "documents",
                "pins",
                "snapshots",
                "files",
                "file_data",
                "notes",
                "links",
                "attachments",
                "records",
                "record_attachments",
                "retention_queue",
                "gc_files",
                "gc_dirty",
                "trash_visible",
                "fulltext",
                "titletext",
                "shorttext",
                "shorttitle",
                "retitles",
                "state",
            ] {
                let exists: bool = db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?)",
                    [table],
                    |row| row.get(0),
                )?;
                if !exists {
                    return Err(Error::InvalidData("The notes database is incomplete."));
                }
            }
            let building = db
                .query_row("SELECT value FROM state WHERE key='building'", [], |row| {
                    row.get::<_, bool>(0)
                })
                .optional()?
                .unwrap_or(false);
            for (kind, names) in [
                ("trigger", &["count_insert", "count_delete"][..]),
                (
                    "index",
                    &[
                        "attachment_sources",
                        "record_name",
                        "record_recent",
                        "record_expiry",
                        "recovery_references",
                        "gc_expiry",
                        "trash_recent",
                        "record_sequence",
                        "record_kind_sequence",
                        "record_unique_sequence",
                    ][..],
                ),
            ] {
                for name in names {
                    if !db.query_row(
                        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type=? AND name=?)",
                        params![kind, name],
                        |row| row.get::<_, bool>(0),
                    )? {
                        return Err(Error::InvalidData(
                            "The notes database schema is incomplete.",
                        ));
                    }
                }
            }
            if !building {
                for name in ["recent_order", "titles", "hubs", "incoming"] {
                    if !db.query_row(
                        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='index' AND name=?)",
                        [name],
                        |row| row.get::<_, bool>(0),
                    )? {
                        return Err(Error::InvalidData(
                            "The notes database indexes are incomplete.",
                        ));
                    }
                }
            }
        }
        super::workspaces::check_schema(&db)?;
        Self::configured(db)
    }

    fn configured(mut db: Connection) -> Result<Self, Error> {
        db.create_scalar_function(
            "note_contains",
            2,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8
                | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
            |context| {
                let text: String = context.get(0)?;
                let term: String = context.get(1)?;
                Ok(text.to_lowercase().contains(&term))
            },
        )?;
        let clock = Instant::now();
        let deadline = Arc::new(AtomicU64::new(u64::MAX));
        let timer = Arc::clone(&deadline);
        db.progress_handler(
            2000,
            Some(move || clock.elapsed().as_millis() as u64 >= timer.load(Ordering::Relaxed)),
        )?;
        Ok(Self {
            db,
            clock,
            deadline,
        })
    }

    pub fn count(&self) -> Result<usize, Error> {
        let count: i64 =
            self.db
                .query_row("SELECT value FROM state WHERE key='count'", [], |row| {
                    row.get(0)
                })?;
        usize::try_from(count).map_err(|_| Error::InvalidData("The catalog count is invalid."))
    }

    pub fn stamp(&self, name: &NoteName) -> Result<Option<String>, Error> {
        Ok(self
            .db
            .query_row(
                "SELECT stamp FROM notes WHERE name=?",
                [name.as_str()],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn put(&self, note: &Note, stamp: &str, pinned: bool, bulk: bool) -> Result<(), Error> {
        let metadata = crate::markdown::index_metadata(note.name.as_str(), &note.body);
        let title = metadata.title.unwrap_or_else(|| {
            note.body
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("Untitled note")
                .trim()
                .trim_start_matches('#')
                .trim()
                .to_owned()
        });
        let summary = Summary {
            name: note.name.clone(),
            title: title.chars().take(200).collect(),
            modified: note.modified,
            pinned,
            tags: metadata
                .tags
                .iter()
                .take(32)
                .map(|tag| tag.chars().take(64).collect())
                .collect(),
            bytes: note.body.len(),
            snippet: note.body.chars().take(160).collect(),
        };
        let text = format!("{}\n{}", note.name.as_str(), note.body).to_lowercase();
        let title = title.to_lowercase();
        let old_targets = if bulk {
            Vec::new()
        } else {
            self.outgoing(&note.name)?
        };
        let old_files = if bulk {
            Vec::new()
        } else {
            self.attachment_links(&note.name)?
        };
        let existed = !bulk && self.stamp(&note.name)?.is_some();
        let modified = i64::try_from(note.modified).map_err(|_| Error::InvalidFile)?;
        let id: i64 = self.db.prepare_cached("INSERT INTO notes(id,name,summary,stamp,title_search,modified,pinned,open_task,tags,outgoing) VALUES((SELECT id FROM notes WHERE name=?1),?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(name) DO UPDATE SET summary=excluded.summary,stamp=excluded.stamp,title_search=excluded.title_search,modified=excluded.modified,pinned=excluded.pinned,open_task=excluded.open_task,tags=excluded.tags,degree=notes.degree+excluded.outgoing-notes.outgoing,outgoing=excluded.outgoing RETURNING id")?.query_row(params![note.name.as_str(),serde_json::to_string(&summary).map_err(data_error)?,stamp,title,modified,pinned,note.body.contains("[ ]"),serde_json::to_string(&metadata.tags).map_err(data_error)?,metadata.links.len() as i64], |row| row.get(0))?;
        self.db
            .prepare_cached("DELETE FROM links WHERE source=?")?
            .execute([id])?;
        self.db
            .prepare_cached("DELETE FROM attachments WHERE source=?")?
            .execute([id])?;
        for target in &metadata.links {
            self.db
                .prepare_cached("INSERT OR IGNORE INTO links VALUES(?,?)")?
                .execute(params![id, target])?;
        }
        let new_files = crate::markdown::attachment_references(&note.body);
        for target in &new_files {
            self.db
                .prepare_cached("INSERT OR IGNORE INTO attachments VALUES(?,?)")?
                .execute(params![id, target])?;
            self.db
                .prepare_cached("UPDATE gc_files SET unused=NULL WHERE id=?")?
                .execute([target])?;
        }
        for target in old_files.iter().chain(&new_files) {
            self.db
                .prepare_cached("INSERT OR IGNORE INTO gc_dirty VALUES(?)")?
                .execute([target])?;
        }
        write_search_text(&self.db, id, SearchField::Body, &text)?;
        write_search_text(&self.db, id, SearchField::Title, &title)?;
        self.db.execute(
            "UPDATE retitles SET ready=1 WHERE target=? AND revision=?",
            params![note.name.as_str(), note.revision.as_str()],
        )?;
        if !bulk {
            let old: BTreeSet<_> = old_targets.into_iter().collect();
            let current: BTreeSet<_> = metadata.links.into_iter().collect();
            for target in old.difference(&current) {
                self.db
                    .prepare_cached("UPDATE notes SET degree=degree-1 WHERE name=?")?
                    .execute([target])?;
            }
            for target in current.difference(&old) {
                self.db
                    .prepare_cached("UPDATE notes SET degree=degree+1 WHERE name=?")?
                    .execute([target])?;
            }
            if !existed {
                self.refresh_degree(note.name.as_str())?;
            }
        }
        Ok(())
    }

    fn refresh_degree(&self, name: &str) -> Result<(), Error> {
        self.db.prepare_cached("UPDATE notes SET degree=outgoing+(SELECT count(*) FROM links WHERE target=?1) WHERE name=?1")?.execute([name])?;
        Ok(())
    }

    pub fn finish_rebuild(&self) -> Result<(), Error> {
        self.db.execute_batch("CREATE INDEX IF NOT EXISTS incoming ON links(target,source); DROP INDEX IF EXISTS hubs; UPDATE notes SET degree=outgoing+(SELECT count(*) FROM links WHERE target=notes.name);")?;
        self.ensure_indexes()?;
        self.db
            .execute("INSERT OR REPLACE INTO state VALUES('building',0)", [])?;
        self.db
            .execute_batch("PRAGMA optimize; PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    pub fn begin_import(&self) -> Result<(), Error> {
        let transaction = self.db.unchecked_transaction()?;
        self.db.execute_batch("INSERT OR REPLACE INTO state VALUES('building',1); DROP INDEX IF EXISTS incoming; DROP INDEX IF EXISTS recent_order; DROP INDEX IF EXISTS titles; DROP INDEX IF EXISTS hubs;")?;
        transaction.commit()?;
        Ok(())
    }

    pub fn ensure_indexes(&self) -> Result<(), Error> {
        self.db.execute_batch("CREATE INDEX IF NOT EXISTS incoming ON links(target,source); CREATE INDEX IF NOT EXISTS recent_order ON notes(pinned DESC,modified DESC,name DESC); CREATE INDEX IF NOT EXISTS titles ON notes(title_search); CREATE INDEX IF NOT EXISTS hubs ON notes(degree DESC,name);")?;
        Ok(())
    }

    pub fn remove(&self, name: &NoteName) -> Result<(), Error> {
        let targets = self.outgoing(name)?;
        for id in self.attachment_links(name)? {
            self.db
                .prepare_cached("INSERT OR IGNORE INTO gc_dirty VALUES(?)")?
                .execute([id])?;
        }
        if let Some(id) = self
            .db
            .query_row(
                "SELECT id FROM notes WHERE name=?",
                [name.as_str()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
        {
            self.db
                .execute("DELETE FROM fulltext WHERE rowid=?", [id])?;
            self.db
                .execute("DELETE FROM titletext WHERE rowid=?", [id])?;
            self.db
                .execute("DELETE FROM shorttext WHERE rowid=?", [id])?;
            self.db
                .execute("DELETE FROM shorttitle WHERE rowid=?", [id])?;
            self.db.execute("DELETE FROM notes WHERE id=?", [id])?;
        }
        for target in targets {
            self.db
                .prepare_cached("UPDATE notes SET degree=degree-1 WHERE name=?")?
                .execute([target])?;
        }
        Ok(())
    }

    pub fn outgoing(&self, name: &NoteName) -> Result<Vec<String>, Error> {
        Ok(self
            .db
            .prepare_cached(
                "SELECT target FROM links WHERE source=(SELECT id FROM notes WHERE name=?)",
            )?
            .query_map([name.as_str()], |row| row.get(0))?
            .collect::<Result<_, _>>()?)
    }

    pub fn incoming(
        &self,
        name: &NoteName,
        after: i64,
        limit: usize,
    ) -> Result<Vec<(i64, NoteName)>, Error> {
        self.db.prepare_cached("SELECT n.id,n.name FROM links l JOIN notes n ON n.id=l.source WHERE l.target=? AND l.source>? ORDER BY l.source LIMIT ?")?.query_map(params![name.as_str(),after,limit as i64], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?.map(|row| { let (id,name)=row?; Ok((id,NoteName::parse(name)?)) }).collect()
    }

    pub fn summary(&self, name: &str) -> Result<Option<Summary>, Error> {
        self.db
            .query_row("SELECT summary FROM notes WHERE name=?", [name], |row| {
                row.get::<_, String>(0)
            })
            .optional()?
            .map(|text| serde_json::from_str(&text).map_err(data_error))
            .transpose()
    }

    pub fn retitle_candidates(
        &self,
        name: &NoteName,
        previous: &str,
        after: i64,
        limit: usize,
    ) -> Result<Vec<(i64, NoteName)>, Error> {
        let escaped = previous
            .replace('\\', "\\\\")
            .replace('[', "\\[")
            .replace(']', "\\]");
        let query = if escaped.contains('\0') {
            "k : set".to_owned()
        } else {
            format!(
                "text : \"[{}](\" OR k : set",
                escaped.to_lowercase().replace('"', "\"\"")
            )
        };
        self.db.prepare_cached("SELECT f.rowid,n.name FROM fulltext f JOIN notes n ON n.id=f.rowid WHERE fulltext MATCH ?1 AND f.rowid>?2 AND EXISTS(SELECT 1 FROM links WHERE source=f.rowid AND target=?3) ORDER BY f.rowid LIMIT ?4")?.query_map(params![query,after,name.as_str(),limit as i64],|row|Ok((row.get::<_,i64>(0)?,row.get::<_,String>(1)?)))?.map(|row|{let(id,name)=row?;Ok((id,NoteName::parse(name)?))}).collect()
    }

    pub fn attachment_links(&self, name: &NoteName) -> Result<Vec<String>, Error> {
        Ok(self
            .db
            .prepare_cached(
                "SELECT target FROM attachments WHERE source=(SELECT id FROM notes WHERE name=?)",
            )?
            .query_map([name.as_str()], |row| row.get(0))?
            .collect::<Result<_, _>>()?)
    }

    pub fn queue_title(
        &self,
        target: &NoteName,
        previous: &str,
        title: &str,
        revision: &str,
    ) -> Result<(), Error> {
        self.db.execute(
            "INSERT INTO retitles(target,previous,title,revision) VALUES(?,?,?,?)",
            params![target.as_str(), previous, title, revision],
        )?;
        Ok(())
    }

    pub fn sources_for_attachment(
        &self,
        target: &str,
        limit: usize,
    ) -> Result<Vec<Summary>, Error> {
        self.db.prepare_cached("SELECT n.summary FROM attachments a JOIN notes n ON n.id=a.source WHERE a.target=? LIMIT ?")?.query_map(params![target,limit as i64], |row| row.get::<_,String>(0))?.map(|row| serde_json::from_str(&row?).map_err(data_error)).collect()
    }

    pub fn search(
        &self,
        query: &str,
        cursor: Option<&str>,
        limit: usize,
        titles: bool,
    ) -> Result<Page, Error> {
        if !(1..=200).contains(&limit) || query.len() > 512 || query.split_whitespace().count() > 32
        {
            return Err(Error::InvalidData(
                "The search query or page size is invalid.",
            ));
        }
        let query = query.trim().to_lowercase();
        let parsed = decode_cursor(&query, cursor, titles)?;
        let prioritize = !query.is_empty()
            && query.split_whitespace().all(|term| {
                !term.starts_with(['-', '#'])
                    && !term.starts_with("tag:")
                    && !term.starts_with("path:")
                    && !matches!(term, "is:pinned" | "task:open")
            });
        if !prioritize {
            if parsed.as_ref().is_some_and(|cursor| cursor.exact) {
                return Err(Error::InvalidData("The search cursor is invalid."));
            }
            return self.search_scan(&query, cursor, limit, titles, false);
        }
        let mut exact = Vec::new();
        let mut last = None;
        if parsed.as_ref().is_none_or(|cursor| cursor.exact) {
            let mut statement=self.db.prepare_cached("SELECT id,summary,pinned,modified,name FROM notes WHERE title_search=? AND id<=? ORDER BY id DESC LIMIT ?")?;
            let mut rows = statement.query(params![
                query,
                parsed.as_ref().map_or(i64::MAX, |cursor| cursor.id - 1),
                (limit + 1) as i64
            ])?;
            while let Some(row) = rows.next()? {
                if exact.len() == limit {
                    return Ok(Page {
                        notes: exact,
                        next: last.map(encode_cursor).transpose()?,
                        total: self.count()?,
                    });
                }
                exact.push(serde_json::from_str(&row.get::<_, String>(1)?).map_err(data_error)?);
                last = Some(Cursor {
                    query: query.clone(),
                    titles,
                    id: row.get(0)?,
                    pinned: row.get(2)?,
                    modified: row.get(3)?,
                    name: row.get(4)?,
                    exact: true,
                });
            }
        }
        let remaining = limit - exact.len();
        let after = if parsed.as_ref().is_some_and(|cursor| !cursor.exact) {
            cursor
        } else {
            None
        };
        let page = self.search_scan(&query, after, remaining.max(1), titles, true)?;
        if remaining == 0 {
            let next = if page.notes.is_empty() {
                None
            } else {
                Some(encode_cursor(Cursor {
                    query,
                    titles,
                    id: i64::MAX,
                    pinned: false,
                    modified: 0,
                    name: String::new(),
                    exact: false,
                })?)
            };
            return Ok(Page {
                notes: exact,
                next,
                total: page.total,
            });
        }
        exact.extend(page.notes);
        Ok(Page {
            notes: exact,
            next: page.next,
            total: page.total,
        })
    }

    fn search_scan(
        &self,
        query: &str,
        cursor: Option<&str>,
        limit: usize,
        titles: bool,
        exclude_exact: bool,
    ) -> Result<Page, Error> {
        if !(1..=200).contains(&limit) || query.len() > 512 {
            return Err(Error::InvalidData(
                "The search query or page size is invalid.",
            ));
        }
        let query = query.trim().to_lowercase();
        let cursor = decode_cursor(&query, cursor, titles)?;
        let mut params: Vec<Value> = Vec::new();
        let mut conditions = Vec::new();
        let mut long_positive = Vec::new();
        let mut long_negative = Vec::new();
        let mut short_positive = Vec::new();
        let mut short_negative = Vec::new();
        let mut compact_positive = Vec::new();
        let mut compact_negative = Vec::new();
        let mut has_long = false;
        let mut indexed = false;
        for term in query.split_whitespace().take(33) {
            if conditions.len() >= 32 {
                return Err(Error::InvalidData("Use at most 32 search terms."));
            }
            let (term, negate) = term
                .strip_prefix('-')
                .map_or((term, false), |term| (term, true));
            if term.is_empty() {
                continue;
            }
            let condition =
                if let Some(tag) = term.strip_prefix('#').or_else(|| term.strip_prefix("tag:")) {
                    params.push(Value::Text(tag.to_owned()));
                    "EXISTS(SELECT 1 FROM json_each(n.tags) WHERE value=?)".to_owned()
                } else if let Some(path) = term.strip_prefix("path:") {
                    params.push(Value::Text(path.to_owned()));
                    "instr(lower(n.name),?)=1".to_owned()
                } else if term == "is:pinned" {
                    "n.pinned=1".to_owned()
                } else if term == "task:open" {
                    "n.open_task=1".to_owned()
                } else {
                    indexed = true;
                    let expression = trigram_expression(term);
                    let length = term.chars().count();
                    if !negate {
                        compact_positive.push(expression.clone());
                    } else if length <= 3 {
                        compact_negative.push(expression.clone());
                    }
                    if length < 3 {
                        if negate {
                            short_negative.push(expression);
                        } else {
                            short_positive.push(expression);
                        }
                    } else {
                        has_long = true;
                        if term.contains('\0') {
                            if !negate {
                                long_positive.push("k : set".into());
                            }
                        } else {
                            let phrase = format!("text : \"{}\"", term.replace('"', "\"\""));
                            if negate {
                                long_negative.push(phrase);
                            } else {
                                long_positive.push(phrase);
                            }
                        }
                    }
                    params.push(Value::Text(term.to_owned()));
                    if !titles {
                        "note_contains(n.name || char(10) || v.body,?)".to_owned()
                    } else {
                        "instr(n.title_search,?)>0".to_owned()
                    }
                };
            conditions.push(if negate {
                format!("NOT ({condition})")
            } else {
                condition
            });
        }
        let positional = if titles { "titletext" } else { "fulltext" };
        let distinct = if titles { "shorttitle" } else { "shorttext" };
        let (table, expression, secondary) = if has_long {
            let native = fts_expression(&long_positive, &long_negative, "k : all");
            let small = fts_expression(&short_positive, &short_negative, "all");
            let large = fts_expression(&compact_positive, &compact_negative, "all");
            (
                positional,
                format!("({native}) OR k : set"),
                Some(format!("(pos AND ({small})) OR (set AND ({large}))")),
            )
        } else {
            (
                distinct,
                fts_expression(&short_positive, &short_negative, "all"),
                None,
            )
        };
        let body_join = if !titles {
            " JOIN documents v ON v.name=n.name"
        } else {
            ""
        };
        let mut sql = if indexed {
            params.insert(0, Value::Text(expression));
            format!(
                "SELECT n.id,n.summary,n.pinned,n.modified,n.name FROM {table} s JOIN notes n ON n.id=s.rowid{body_join} WHERE {table} MATCH ?"
            )
        } else {
            "SELECT n.id,n.summary,n.pinned,n.modified,n.name FROM notes n WHERE 1=1".to_owned()
        };
        for condition in conditions {
            sql.push_str(" AND (");
            sql.push_str(&condition);
            sql.push(')');
        }
        if let Some(expression) = secondary {
            sql.push_str(&format!(
                " AND EXISTS(SELECT 1 FROM {distinct} WHERE rowid=n.id AND {distinct} MATCH ?)"
            ));
            params.push(Value::Text(expression));
        }
        if exclude_exact {
            sql.push_str(" AND n.title_search!=?");
            params.push(Value::Text(query.clone()));
        }
        if let Some(cursor) = &cursor {
            if indexed {
                sql.push_str(" AND s.rowid<?");
                params.push(Value::Integer(cursor.id));
            } else {
                sql.push_str(" AND (n.pinned,n.modified,n.name)<(?,?,?)");
                params.extend([
                    Value::Integer(i64::from(cursor.pinned)),
                    Value::Integer(cursor.modified),
                    Value::Text(cursor.name.clone()),
                ]);
            }
        }
        sql.push_str(if indexed {
            " ORDER BY s.rowid DESC LIMIT ?"
        } else {
            " ORDER BY n.pinned DESC,n.modified DESC,n.name DESC LIMIT ?"
        });
        params.push(Value::Integer((limit + 1) as i64));
        self.deadline.store(
            self.clock.elapsed().as_millis() as u64 + 750,
            Ordering::Relaxed,
        );
        let result = (|| -> Result<_, Error> {
            let mut stmt = self.db.prepare(&sql)?;
            let mut rows = stmt.query(params_from_iter(params))?;
            let mut notes = Vec::new();
            let mut last = None;
            let mut more = false;
            while let Some(row) = rows.next()? {
                if notes.len() == limit {
                    more = true;
                    break;
                }
                let note: Summary =
                    serde_json::from_str(&row.get::<_, String>(1)?).map_err(data_error)?;
                last = Some(Cursor {
                    query: query.clone(),
                    titles,
                    id: row.get(0)?,
                    pinned: row.get(2)?,
                    modified: row.get(3)?,
                    name: row.get(4)?,
                    exact: false,
                });
                notes.push(note);
            }
            let next = if more {
                last.map(|cursor| {
                    serde_json::to_vec(&cursor)
                        .map(|bytes| BASE64_URL_SAFE_NO_PAD.encode(bytes))
                        .map_err(data_error)
                })
                .transpose()?
            } else {
                None
            };
            Ok(Page {
                notes,
                next,
                total: self.count()?,
            })
        })();
        self.deadline.store(u64::MAX, Ordering::Relaxed);
        match result {
            Err(Error::Catalog(rusqlite::Error::SqliteFailure(error, _)))
                if error.code == rusqlite::ErrorCode::OperationInterrupted =>
            {
                Err(Error::SearchLimit)
            }
            result => result,
        }
    }

    pub fn graph(&self, focus: Option<&NoteName>, limit: usize) -> Result<Graph, Error> {
        if !(1..=120).contains(&limit) {
            return Err(Error::InvalidData(
                "The graph size must be between 1 and 120.",
            ));
        }
        let mut names = Vec::new();
        let mut neighbors = 0;
        if let Some(focus) = focus {
            if self.summary(focus.as_str())?.is_some() {
                names.push(focus.as_str().to_owned());
            }
            let mut outgoing=self.db.prepare_cached("SELECT n.name FROM links l JOIN notes n ON n.name=l.target WHERE l.source=(SELECT id FROM notes WHERE name=?) AND n.name!=? ORDER BY n.degree DESC,n.name LIMIT ?")?.query_map(params![focus.as_str(),focus.as_str(),limit as i64], |row| row.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
            let incoming = self.incoming(focus, 0, limit)?;
            outgoing.extend(
                incoming
                    .into_iter()
                    .map(|(_, name)| name.as_str().to_owned()),
            );
            neighbors=self.db.query_row("SELECT (SELECT count(*) FROM links WHERE target=?1 AND source!=(SELECT id FROM notes WHERE name=?1))+(SELECT count(*) FROM links l JOIN notes n ON n.name=l.target WHERE l.source=(SELECT id FROM notes WHERE name=?1) AND l.target!=?1)-(SELECT count(*) FROM links l JOIN notes n ON n.name=l.target JOIN links back ON back.source=n.id AND back.target=?1 WHERE l.source=(SELECT id FROM notes WHERE name=?1) AND l.target!=?1)", [focus.as_str()], |row| row.get::<_,i64>(0))? as usize;
            for name in outgoing {
                if names.len() >= limit {
                    break;
                }
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        if names.is_empty() {
            names = self
                .db
                .prepare_cached("SELECT name FROM notes ORDER BY degree DESC,name LIMIT ?")?
                .query_map([limit as i64], |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?;
        }
        let mut notes = Vec::new();
        for name in &names {
            if let Some(note) = self.summary(name)? {
                notes.push(note);
            }
        }
        let selected: BTreeSet<_> = names.iter().collect();
        let mut edges = Vec::new();
        let mut truncated = neighbors + usize::from(focus.is_some()) > notes.len();
        for note in &notes {
            let mut stmt = self.db.prepare_cached(
                "SELECT target FROM links WHERE source=(SELECT id FROM notes WHERE name=?)",
            )?;
            let mut rows = stmt.query([note.name.as_str()])?;
            while let Some(row) = rows.next()? {
                let target: String = row.get(0)?;
                if selected.contains(&target) {
                    if edges.len() >= 2048 {
                        truncated = true;
                        break;
                    }
                    edges.push(Edge {
                        source: note.name.as_str().to_owned(),
                        target,
                    });
                }
            }
        }
        Ok(Graph {
            notes,
            edges,
            total: self.count()?,
            neighbors,
            truncated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_nonmatching_phrases_do_not_read_original_bodies() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = super::super::Store::open(directory.path()).unwrap();
        store
            .create(Some("One.md".into()), "# Topic\n\nabc bcd bca cab")
            .unwrap();
        let calls = Arc::new(AtomicU64::new(0));
        let observed = Arc::clone(&calls);
        store
            .index
            .db
            .create_scalar_function(
                "note_contains",
                2,
                rusqlite::functions::FunctionFlags::SQLITE_UTF8
                    | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
                move |context| {
                    observed.fetch_add(1, Ordering::Relaxed);
                    Ok(context
                        .get::<String>(0)?
                        .to_lowercase()
                        .contains(&context.get::<String>(1)?))
                },
            )
            .unwrap();
        for query in ["abcd", "abcabc"] {
            assert!(
                store
                    .search(query, None, 10, false)
                    .unwrap()
                    .notes
                    .is_empty()
            );
        }
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        let note = store
            .read(&NoteName::parse("One.md".into()).unwrap())
            .unwrap();
        let larger = store
            .update(
                &note.name,
                &format!("# Topic\n\nabc bcd bca cab {}", "x".repeat(16 * 1024)),
                note.revision.as_str(),
            )
            .unwrap();
        assert!(
            store
                .search("abcd", None, 10, false)
                .unwrap()
                .notes
                .is_empty()
        );
        assert!(calls.load(Ordering::Relaxed) > 0);
        store
            .update(
                &larger.name,
                "# Topic\n\nabc bcd bca cab",
                larger.revision.as_str(),
            )
            .unwrap();
        calls.store(0, Ordering::Relaxed);
        assert!(
            store
                .search("abcd", None, 10, false)
                .unwrap()
                .notes
                .is_empty()
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn large_substring_indexes_omit_body_positions() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = super::super::Store::open(directory.path()).unwrap();
        store
            .create(
                Some("One.md".into()),
                &format!(
                    "# {}\n\n{}",
                    "Repeated title ".repeat(1400),
                    "repeat ".repeat(4000)
                ),
            )
            .unwrap();
        for table in ["fulltext", "titletext"] {
            store.index.db.execute_batch(&format!(
                "CREATE VIRTUAL TABLE temp.positions USING fts5vocab(main,'{table}','instance');"
            )).unwrap();
            let (tokens, positions): (i64, i64) = store
                .index
                .db
                .query_row(
                    "SELECT count(*),count(CASE WHEN col='text' THEN offset END) FROM positions",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert!(tokens > 0);
            assert_eq!(positions, 0, "{table}");
            store
                .index
                .db
                .execute_batch("DROP TABLE temp.positions;")
                .unwrap();
        }
    }

    #[test]
    fn database_working_memory_uses_bounded_pages_and_disk_temporaries() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(&directory.path().join("notes.sqlite")).unwrap();
        for (pragma, expected) in [("cache_size", -2048), ("temp_store", 1), ("mmap_size", 0)] {
            assert_eq!(
                catalog
                    .db
                    .pragma_query_value(None, pragma, |row| row.get::<_, i64>(0))
                    .unwrap(),
                expected,
                "{pragma}"
            );
        }
    }

    #[test]
    fn streamed_search_grams_match_unicode_windows_and_duplicate_elimination() {
        for text in [
            "",
            "a",
            "aa",
            "ababa",
            "日本語😀 日本語",
            "\u{10ffff}\u{10000}é\n",
        ] {
            let characters: Vec<_> = text.chars().collect();
            let mut keys = BTreeSet::new();
            for size in 1..=3 {
                for window in characters.windows(size) {
                    keys.insert(gram_key(window));
                }
            }
            let mut expected = String::new();
            for key in keys {
                write!(expected, "g{key:x} ").unwrap();
            }
            expected.push_str("all");
            assert_eq!(grams(text, true), expected);
        }
    }

    #[test]
    fn an_index_publication_failure_keeps_the_import_resumable() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(&directory.path().join("notes.sqlite")).unwrap();
        catalog.begin_import().unwrap();
        catalog
            .db
            .execute_batch(
                "INSERT OR REPLACE INTO state VALUES('building',1); CREATE TABLE hubs(value TEXT);",
            )
            .unwrap();
        assert!(catalog.finish_rebuild().is_err());
        assert!(
            catalog
                .db
                .query_row("SELECT value FROM state WHERE key='building'", [], |row| {
                    row.get::<_, bool>(0)
                })
                .unwrap()
        );
        catalog.db.execute_batch("DROP TABLE hubs;").unwrap();
        catalog.finish_rebuild().unwrap();
        assert!(
            !catalog
                .db
                .query_row("SELECT value FROM state WHERE key='building'", [], |row| {
                    row.get::<_, bool>(0)
                })
                .unwrap()
        );
    }
}
