use crate::{
    Error,
    domain::{NoteName, Revision},
    store::{MAX_ATTACHMENT_BYTES, MAX_NOTE_BYTES},
};
use rustix::fs::{self, Mode, OFlags, RenameFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, BufRead, BufReader, Read, Seek, Write},
    path::{Path, PathBuf},
};

const MAX_METADATA_BYTES: u64 = 16 * 1024 * 1024;

pub(crate) fn invalid() -> Error {
    Error::InvalidData("The backup format or payload is invalid.")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Document,
    Pin,
    Snapshot,
    File,
    FileClock,
    TitleUpdate,
    State,
    Workspace,
    Membership,
    WorkspaceState,
    Sequence,
    Retention,
    DirtyFile,
}

impl Kind {
    pub const ALL: [Self; 13] = [
        Self::Document,
        Self::Pin,
        Self::Snapshot,
        Self::File,
        Self::FileClock,
        Self::TitleUpdate,
        Self::State,
        Self::Workspace,
        Self::Membership,
        Self::WorkspaceState,
        Self::Sequence,
        Self::Retention,
        Self::DirtyFile,
    ];

    pub fn table(self) -> &'static str {
        match self {
            Self::Document => "documents",
            Self::Pin => "pins",
            Self::Snapshot => "snapshots",
            Self::File => "files",
            Self::FileClock => "gc_files",
            Self::TitleUpdate => "retitles",
            Self::State => "state",
            Self::Workspace => "workspaces",
            Self::Membership => "workspace_members",
            Self::WorkspaceState => "workspace_state",
            Self::Sequence => "sqlite_sequence",
            Self::Retention => "retention_queue",
            Self::DirtyFile => "gc_dirty",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Cell {
    Text(String),
    Integer(i64),
    Bytes(Vec<u8>),
    Null,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub kind: Kind,
    pub values: Vec<Cell>,
}

impl Record {
    pub fn text(&self, index: usize) -> Result<&str, Error> {
        match self.values.get(index) {
            Some(Cell::Text(value)) => Ok(value),
            _ => Err(invalid()),
        }
    }

    pub fn integer(&self, index: usize) -> Result<i64, Error> {
        match self.values.get(index) {
            Some(Cell::Integer(value)) => Ok(*value),
            _ => Err(invalid()),
        }
    }

    pub fn bytes(&self, index: usize) -> Result<&[u8], Error> {
        match self.values.get(index) {
            Some(Cell::Bytes(value)) => Ok(value),
            _ => Err(invalid()),
        }
    }

    pub fn validate(&self) -> Result<(), Error> {
        let shape = match self.kind {
            Kind::Document => "ttii",
            Kind::Pin | Kind::Retention | Kind::DirtyFile => "t",
            Kind::Snapshot => "itttiii",
            Kind::File => "tbbttii",
            Kind::FileClock => "ttn",
            Kind::TitleUpdate => "ittttii",
            Kind::State | Kind::Sequence => "ti",
            Kind::Workspace => "iti",
            Kind::Membership => "iit",
            Kind::WorkspaceState => "ii",
        };
        if shape.len() != self.values.len()
            || !shape.bytes().zip(&self.values).all(|(shape, cell)| {
                matches!(
                    (shape, cell),
                    (b't', Cell::Text(_))
                        | (b'i', Cell::Integer(_))
                        | (b'b', Cell::Bytes(_))
                        | (b'n', Cell::Integer(_) | Cell::Null)
                )
            })
        {
            return Err(invalid());
        }
        if self
            .values
            .iter()
            .any(|cell| matches!(cell, Cell::Integer(value) if *value < 0))
        {
            return Err(invalid());
        }
        match self.kind {
            Kind::Document => {
                NoteName::parse(self.text(0)?.into())?;
                Revision::parse(self.text(1)?.into())?;
                if self.integer(3)? == 0 {
                    return Err(invalid());
                }
            }
            Kind::Pin | Kind::Retention => {
                NoteName::parse(self.text(0)?.into())?;
            }
            Kind::Snapshot => {
                if self.integer(0)? > 1
                    || !crate::rules::valid_id(self.text(1)?.as_bytes())
                    || self.integer(6)? == 0
                {
                    return Err(invalid());
                }
                NoteName::parse(self.text(2)?.into())?;
                Revision::parse(self.text(3)?.into())?;
            }
            Kind::File => {
                let metadata: crate::store::AttachmentMeta =
                    serde_json::from_slice(self.bytes(1)?).map_err(|_| invalid())?;
                metadata.validate()?;
                if !crate::rules::valid_attachment(self.text(0)?.as_bytes())
                    || self.bytes(2)?.len() > 512
                    || self.integer(6)? > MAX_ATTACHMENT_BYTES as i64
                {
                    return Err(invalid());
                }
                Revision::parse(self.text(3)?.into())?;
                Revision::parse(self.text(4)?.into())?;
            }
            Kind::FileClock | Kind::DirtyFile => {
                if !crate::rules::valid_attachment(self.text(0)?.as_bytes()) {
                    return Err(invalid());
                }
            }
            Kind::TitleUpdate => {
                if self.integer(0)? == 0 || self.integer(6)? > 1 {
                    return Err(invalid());
                }
                NoteName::parse(self.text(1)?.into())?;
                Revision::parse(self.text(4)?.into())?;
            }
            Kind::State => {
                if !matches!(
                    self.text(0)?,
                    "count" | "record-sequence" | "trash-purge" | "building"
                ) || (self.text(0)? == "building" && self.integer(1)? != 0)
                {
                    return Err(invalid());
                }
            }
            Kind::Workspace => {
                let name = self.text(1)?;
                if self.integer(0)? == 0
                    || name.trim() != name
                    || name.is_empty()
                    || name.chars().count() > 120
                    || name.len() > 480
                    || name
                        .chars()
                        .any(|ch| ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}'))
                {
                    return Err(invalid());
                }
            }
            Kind::Membership => {
                if self.integer(0)? == 0 || self.integer(1)? == 0 {
                    return Err(invalid());
                }
                NoteName::parse(self.text(2)?.into())?;
            }
            Kind::WorkspaceState => {
                if self.integer(0)? != 1 {
                    return Err(invalid());
                }
            }
            Kind::Sequence => {
                if !matches!(
                    self.text(0)?,
                    "notes" | "retitles" | "workspaces" | "workspace_members"
                ) {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }

    fn payload(&self) -> Result<Option<(String, usize, &str)>, Error> {
        Ok(match self.kind {
            Kind::Document => Some((
                format!("notes/{:016x}.md", self.integer(3)?),
                MAX_NOTE_BYTES,
                self.text(1)?,
            )),
            Kind::Snapshot => Some((
                format!("recovery/{}/{}.md", self.integer(0)?, self.text(1)?),
                MAX_NOTE_BYTES,
                self.text(3)?,
            )),
            Kind::File => Some((
                format!("files/{}", self.text(0)?),
                self.integer(6)? as usize,
                self.text(4)?,
            )),
            _ => None,
        })
    }
}

pub struct StagedDirectory {
    temporary: Option<tempfile::TempDir>,
    path: PathBuf,
    destination: PathBuf,
}

impl StagedDirectory {
    pub fn new(destination: &Path, source: &Path) -> Result<Self, Error> {
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .canonicalize()?;
        if parent.starts_with(source.canonicalize()?) {
            return Err(invalid());
        }
        let destination = parent.join(destination.file_name().ok_or_else(invalid)?);
        match destination.symlink_metadata() {
            Ok(_) => return Err(Error::Exists),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let temporary = tempfile::Builder::new()
            .prefix(".tungsten-restore-")
            .tempdir_in(parent)?;
        Ok(Self {
            path: temporary.path().to_owned(),
            temporary: Some(temporary),
            destination,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn publish(mut self) -> Result<(), Error> {
        File::open(self.path())?.sync_all()?;
        fs::renameat_with(
            fs::CWD,
            self.path(),
            fs::CWD,
            &self.destination,
            RenameFlags::NOREPLACE,
        )
        .map_err(io::Error::from)?;
        if let Some(temporary) = self.temporary.take() {
            let _ = temporary.keep();
        }
        File::open(self.destination.parent().ok_or_else(invalid)?)?.sync_all()?;
        Ok(())
    }
}

impl Drop for StagedDirectory {
    fn drop(&mut self) {
        if let Some(temporary) = self.temporary.take()
            && let Err(error) = temporary.close()
        {
            eprintln!("operation=backup_cleanup outcome=failed error={error}");
        }
    }
}

fn root(path: &Path) -> Result<File, Error> {
    Ok(File::from(
        fs::open(
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(io::Error::from)?,
    ))
}

fn open(root: &File, path: &str, create: bool) -> Result<File, Error> {
    let mut parent = root.try_clone()?;
    let mut parts = path.split('/').peekable();
    while let Some(part) = parts.next() {
        if part.is_empty() || matches!(part, "." | "..") {
            return Err(invalid());
        }
        if parts.peek().is_some() {
            if create {
                match fs::mkdirat(&parent, part, Mode::RWXU) {
                    Ok(()) => parent.sync_all()?,
                    Err(rustix::io::Errno::EXIST) => {}
                    Err(error) => return Err(io::Error::from(error).into()),
                }
            }
            parent = File::from(
                fs::openat(
                    &parent,
                    part,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(io::Error::from)?,
            );
        } else {
            let flags = if create {
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL
            } else {
                OFlags::RDONLY
            };
            let file = File::from(
                fs::openat(
                    &parent,
                    part,
                    flags | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                    Mode::RUSR | Mode::WUSR,
                )
                .map_err(io::Error::from)?,
            );
            use std::os::unix::fs::MetadataExt;
            let metadata = file.metadata()?;
            if !metadata.is_file() || metadata.nlink() != 1 {
                return Err(invalid());
            }
            if create {
                parent.sync_all()?;
            }
            return Ok(file);
        }
    }
    Err(invalid())
}

pub fn copy_payload(
    record: &Record,
    source: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<usize, Error> {
    record.validate()?;
    let (_, limit, expected) = record.payload()?.ok_or_else(invalid)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut head = Vec::new();
    let mut length = 0usize;
    loop {
        let count = source.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        length = length
            .checked_add(count)
            .filter(|length| *length <= limit)
            .ok_or_else(invalid)?;
        if head.len() < 512 {
            head.extend_from_slice(&buffer[..count.min(512 - head.len())]);
        }
        digest.update(&buffer[..count]);
        output.write_all(&buffer[..count])?;
    }
    if !Revision::from_digest(digest.clone()).matches(expected) {
        return Err(invalid());
    }
    if record.kind == Kind::File {
        digest.update(record.bytes(1)?);
        if length != limit
            || head != record.bytes(2)?
            || !Revision::from_digest(digest).matches(record.text(3)?)
        {
            return Err(invalid());
        }
    }
    Ok(length)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: u32,
    records: u64,
    metadata_revision: Revision,
}

pub struct Writer {
    root: File,
    metadata: File,
    digest: Sha256,
    records: u64,
}

impl Writer {
    pub fn new(path: &Path) -> Result<Self, Error> {
        let root = root(path)?;
        let metadata = open(&root, "metadata.jsonl", true)?;
        Ok(Self {
            root,
            metadata,
            digest: Sha256::new(),
            records: 0,
        })
    }

    pub fn record(&mut self, record: &Record, source: Option<&mut dyn Read>) -> Result<(), Error> {
        record.validate()?;
        match (record.payload()?, source) {
            (Some((path, _, _)), Some(source)) => {
                let mut output = open(&self.root, &path, true)?;
                copy_payload(record, source, &mut output)?;
                output.sync_all()?;
            }
            (None, None) => {}
            _ => return Err(invalid()),
        }
        let mut line = serde_json::to_vec(record).map_err(|_| invalid())?;
        line.push(b'\n');
        if line.len() as u64 > MAX_METADATA_BYTES {
            return Err(invalid());
        }
        self.metadata.write_all(&line)?;
        self.digest.update(&line);
        self.records = self.records.checked_add(1).ok_or_else(invalid)?;
        Ok(())
    }

    pub fn finish(self) -> Result<(), Error> {
        self.metadata.sync_all()?;
        let manifest = Manifest {
            format: 1,
            records: self.records,
            metadata_revision: Revision::from_digest(self.digest),
        };
        let mut output = open(&self.root, "manifest.json", true)?;
        serde_json::to_writer(&mut output, &manifest).map_err(|_| invalid())?;
        output.sync_all()?;
        self.root.sync_all()?;
        Ok(())
    }
}

pub struct Reader {
    root: File,
    metadata: BufReader<File>,
    digest: Sha256,
    records: u64,
    manifest: Manifest,
    finished: bool,
}

impl Reader {
    pub fn open(path: &Path) -> Result<Self, Error> {
        let root = root(path)?;
        let file = open(&root, "manifest.json", false)?;
        if file.metadata()?.len() > 1024 {
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        file.take(1025).read_to_end(&mut bytes)?;
        if bytes.len() > 1024 {
            return Err(invalid());
        }
        let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if manifest.format != 1 {
            return Err(invalid());
        }
        let metadata = BufReader::new(open(&root, "metadata.jsonl", false)?);
        Ok(Self {
            root,
            metadata,
            digest: Sha256::new(),
            records: 0,
            manifest,
            finished: false,
        })
    }

    pub fn next_record(&mut self) -> Result<Option<Record>, Error> {
        if self.finished {
            return Ok(None);
        }
        let mut line = Vec::new();
        self.metadata
            .by_ref()
            .take(MAX_METADATA_BYTES + 1)
            .read_until(b'\n', &mut line)?;
        if line.is_empty() {
            if self.records != self.manifest.records
                || Revision::from_digest(self.digest.clone()) != self.manifest.metadata_revision
            {
                return Err(invalid());
            }
            self.finished = true;
            return Ok(None);
        }
        if line.len() as u64 > MAX_METADATA_BYTES
            || line.last() != Some(&b'\n')
            || self.records >= self.manifest.records
        {
            return Err(invalid());
        }
        let record: Record = serde_json::from_slice(&line).map_err(|_| invalid())?;
        record.validate()?;
        self.digest.update(&line);
        self.records += 1;
        Ok(Some(record))
    }

    pub fn payload(&self, record: &Record) -> Result<File, Error> {
        record.validate()?;
        let (path, _, _) = record.payload()?.ok_or_else(invalid)?;
        open(&self.root, &path, false)
    }

    pub fn rewind(&mut self) -> Result<(), Error> {
        self.metadata.rewind()?;
        self.digest = Sha256::new();
        self.records = 0;
        self.finished = false;
        Ok(())
    }
}
