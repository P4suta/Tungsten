use crate::{APP_ID, Error, MAX_NOTE_BYTES, NoteName, Result, Revision, types::ServerAddress};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingWrite {
    pub body: String,
    pub revision: Option<Revision>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Draft {
    pub format: u8,
    pub server: ServerAddress,
    pub name: NoteName,
    pub body: String,
    pub revision: Option<Revision>,
    pub pending: Option<PendingWrite>,
}

impl Draft {
    pub fn validate(&self) -> Result<()> {
        if self.format != 1 {
            return Err(Error::Recovery);
        }
        if self.body.len() > MAX_NOTE_BYTES
            || self
                .pending
                .as_ref()
                .is_some_and(|value| value.body.len() > MAX_NOTE_BYTES)
        {
            return Err(Error::TooLarge);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Recovery {
    pub drafts: Vec<Draft>,
    pub damaged: Vec<PathBuf>,
}

#[derive(Clone)]
pub struct DraftStore {
    directory: PathBuf,
    directory_sync: Arc<DirectorySync>,
}

type DirectorySync = dyn Fn(&Path) -> std::io::Result<()> + Send + Sync;

fn digest(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl DraftStore {
    pub fn at(directory: PathBuf) -> Self {
        Self::with_directory_sync(directory, sync_directory)
    }

    pub fn with_directory_sync(
        directory: PathBuf,
        directory_sync: impl Fn(&Path) -> std::io::Result<()> + Send + Sync + 'static,
    ) -> Self {
        Self {
            directory,
            directory_sync: Arc::new(directory_sync),
        }
    }

    pub fn xdg() -> Result<Self> {
        let root = match std::env::var_os("XDG_DATA_HOME") {
            Some(value) if Path::new(&value).is_absolute() => PathBuf::from(value),
            _ => PathBuf::from(
                std::env::var_os("HOME")
                    .ok_or_else(|| Error::Storage(std::io::Error::other("HOME is unavailable")))?,
            )
            .join(".local/share"),
        };
        Ok(Self::at(root.join(APP_ID).join("drafts")))
    }

    pub fn remembered_server(&self) -> Result<Option<ServerAddress>> {
        match fs::read(self.directory.join("server.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| Error::Recovery),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(Error::Storage(error)),
        }
    }

    pub fn remember_server(&self, server: &ServerAddress) -> Result<()> {
        private_directory(&self.directory, self.directory_sync.as_ref())?;
        let bytes = serde_json::to_vec(server).map_err(|_| Error::Recovery)?;
        atomic(
            &self.directory.join("server.json"),
            &bytes,
            self.directory_sync.as_ref(),
        )
    }

    fn server_directory(&self, server: &ServerAddress) -> PathBuf {
        self.directory.join(digest(server.as_str().as_bytes()))
    }

    fn path(&self, server: &ServerAddress, name: &NoteName) -> PathBuf {
        self.server_directory(server)
            .join(format!("{}.json", digest(name.as_str().as_bytes())))
    }

    pub fn save(&self, draft: &Draft) -> Result<()> {
        draft.validate()?;
        let directory = self.server_directory(&draft.server);
        private_directory(&directory, self.directory_sync.as_ref())?;
        let path = self.path(&draft.server, &draft.name);
        self.keep_damaged(&path, &draft.server, &draft.name)?;
        let bytes = serde_json::to_vec(draft).map_err(|_| Error::Recovery)?;
        atomic(&path, &bytes, self.directory_sync.as_ref())
    }

    fn keep_damaged(&self, path: &Path, server: &ServerAddress, name: &NoteName) -> Result<()> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(Error::Storage(error)),
        };
        let valid = metadata.is_file()
            && metadata.len() <= (MAX_NOTE_BYTES * 12 + 4096) as u64
            && fs::read(path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Draft>(&bytes).ok())
                .is_some_and(|draft| {
                    draft.validate().is_ok() && &draft.server == server && &draft.name == name
                });
        if !valid {
            let preserved =
                path.with_file_name(format!("damaged-{}.json", uuid::Uuid::new_v4().simple()));
            fs::rename(path, preserved).map_err(Error::Storage)?;
            (self.directory_sync)(path.parent().ok_or(Error::Recovery)?).map_err(Error::Storage)?;
        }
        Ok(())
    }

    pub fn remove(&self, server: &ServerAddress, name: &NoteName) -> Result<()> {
        let path = self.path(server, name);
        self.keep_damaged(&path, server, name)?;
        match fs::remove_file(&path) {
            Ok(()) => {
                (self.directory_sync)(path.parent().ok_or(Error::Recovery)?).map_err(Error::Storage)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(Error::Storage(error)),
        }
    }

    pub fn recover(&self, server: &ServerAddress) -> Result<Recovery> {
        let directory = self.server_directory(server);
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Recovery {
                    drafts: Vec::new(),
                    damaged: Vec::new(),
                });
            }
            Err(error) => return Err(Error::Storage(error)),
        };
        let mut recovery = Recovery {
            drafts: Vec::new(),
            damaged: Vec::new(),
        };
        for entry in entries {
            let entry = entry.map_err(Error::Storage)?;
            let path = entry.path();
            if path.extension().is_none_or(|value| value != "json") {
                continue;
            }
            let draft = (|| -> Result<Draft> {
                let metadata = fs::symlink_metadata(&path).map_err(Error::Storage)?;
                if !metadata.is_file() || metadata.len() > (MAX_NOTE_BYTES * 12 + 4096) as u64 {
                    return Err(Error::Recovery);
                }
                let bytes = fs::read(&path).map_err(Error::Storage)?;
                let draft: Draft = serde_json::from_slice(&bytes).map_err(|_| Error::Recovery)?;
                draft.validate()?;
                if &draft.server != server || self.path(server, &draft.name) != path {
                    return Err(Error::Recovery);
                }
                Ok(draft)
            })();
            match draft {
                Ok(draft) => recovery.drafts.push(draft),
                Err(_) => recovery.damaged.push(path),
            }
        }
        recovery
            .drafts
            .sort_by(|left, right| left.name.as_str().cmp(right.name.as_str()));
        Ok(recovery)
    }
}

fn sync_directory(directory: &Path) -> std::io::Result<()> {
    fs::File::open(directory).and_then(|file| file.sync_all())
}

fn private_directory(directory: &Path, synchronize: &DirectorySync) -> Result<()> {
    let mut absent = Vec::new();
    let mut parent = directory;
    while !parent.exists() {
        absent.push(parent.to_owned());
        parent = parent.parent().ok_or(Error::Recovery)?;
    }
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(directory).map_err(Error::Storage)?;
    for path in absent {
        synchronize(path.parent().ok_or(Error::Recovery)?).map_err(Error::Storage)?;
    }
    Ok(())
}

fn atomic(path: &Path, bytes: &[u8], synchronize: &DirectorySync) -> Result<()> {
    let directory = path.parent().ok_or(Error::Recovery)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory).map_err(Error::Storage)?;
    temporary.write_all(bytes).map_err(Error::Storage)?;
    temporary.as_file().sync_all().map_err(Error::Storage)?;
    temporary
        .persist(path)
        .map_err(|error| Error::Storage(error.error))?;
    synchronize(directory).map_err(Error::Storage)
}
