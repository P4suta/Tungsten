use crate::{Error, domain::NoteName, store::Store};
use rustix::fs::{self, Mode, OFlags, RenameFlags};
use std::{
    fs::{DirBuilder, File},
    io::{self, Write},
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
};

struct Staging {
    path: PathBuf,
    published: bool,
}

impl Drop for Staging {
    fn drop(&mut self) {
        if !self.published
            && let Err(error) = std::fs::remove_dir_all(&self.path)
        {
            eprintln!("operation=export_cleanup outcome=failed error={error}");
        }
    }
}

fn directory(parent: &File, name: &str) -> Result<File, Error> {
    match fs::mkdirat(parent, name, Mode::RWXU) {
        Ok(()) => parent.sync_all()?,
        Err(rustix::io::Errno::EXIST) => {}
        Err(error) => return Err(Error::Io(error.into())),
    }
    Ok(File::from(
        fs::openat(
            parent,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(io::Error::from)?,
    ))
}

fn file(parent: &File, name: &str) -> Result<File, Error> {
    Ok(File::from(
        fs::openat(
            parent,
            name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(io::Error::from)?,
    ))
}

pub fn write(source: &Path, output: &Path) -> Result<(), Error> {
    let store = Store::open(source)?;
    let source = source.canonicalize()?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    let output = parent.join(output.file_name().ok_or(Error::InvalidFile)?);
    if parent.starts_with(&source) || output.symlink_metadata().is_ok() {
        return Err(Error::InvalidData(
            "Export requires an unused destination outside the data directory.",
        ));
    }
    let path = parent.join(format!(
        ".tungsten-export-{}",
        uuid::Uuid::new_v4().simple()
    ));
    DirBuilder::new().mode(0o700).create(&path)?;
    let mut staging = Staging {
        path,
        published: false,
    };
    let root = File::from(
        fs::open(
            &staging.path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(io::Error::from)?,
    );
    let attachments = directory(&root, ".attachments")?;
    let mut cursor = None;
    loop {
        let page = store.search("", cursor.as_deref(), 200, false)?;
        for summary in page.notes {
            let note = store.read(&summary.name)?;
            let name = NoteName::parse(note.name.as_str().to_owned())?;
            let mut parts = name.as_str().split('/').peekable();
            let mut destination = root.try_clone()?;
            while let Some(part) = parts.next() {
                if parts.peek().is_some() {
                    destination = directory(&destination, part)?;
                } else {
                    let mut file = file(&destination, part)?;
                    file.write_all(note.body.as_bytes())?;
                    file.sync_all()?;
                    destination.sync_all()?;
                }
            }
            for id in crate::markdown::attachment_references(&note.body) {
                let attachment = match store.attachment(&id) {
                    Ok(attachment) => attachment,
                    Err(Error::NotFound) => continue,
                    Err(error) => return Err(error),
                };
                let mut output = match file(&attachments, &id) {
                    Ok(file) => file,
                    Err(Error::Io(error)) if error.kind() == io::ErrorKind::AlreadyExists => {
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                for start in (0..attachment.bytes).step_by(64 * 1024) {
                    output.write_all(&store.attachment_range(
                        &id,
                        start..(start + 64 * 1024).min(attachment.bytes),
                    )?)?;
                }
                output.sync_all()?;
            }
        }
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    attachments.sync_all()?;
    root.sync_all()?;
    fs::renameat_with(
        fs::CWD,
        &staging.path,
        fs::CWD,
        &output,
        RenameFlags::NOREPLACE,
    )
    .map_err(io::Error::from)?;
    staging.published = true;
    File::open(parent)?.sync_all()?;
    Ok(())
}
