use crate::{output, run as command};
use anyhow::{Context, Result, ensure};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

trait Docker {
    fn run(&self, arguments: &[&str]) -> Result<()>;
    fn output(&self, arguments: &[&str]) -> Result<String>;
}

struct Client<'a>(&'a Path);

impl Docker for Client<'_> {
    fn run(&self, arguments: &[&str]) -> Result<()> {
        command(self.0, "docker", arguments)
    }

    fn output(&self, arguments: &[&str]) -> Result<String> {
        output(self.0, "docker", arguments)
    }
}

fn parent(root: &Path, current: &Path, target: Option<&OsStr>) -> PathBuf {
    target.map_or_else(|| root.join("target"), |target| current.join(target))
}

pub fn directory(root: &Path, prefix: &str) -> Result<PathBuf> {
    let target = parent(
        root,
        &std::env::current_dir()?,
        std::env::var_os("CARGO_TARGET_DIR").as_deref(),
    );
    create_directory(&target, prefix)
}

fn create_directory(target: &Path, prefix: &str) -> Result<PathBuf> {
    fs::create_dir_all(target)?;
    let mut builder = tempfile::Builder::new();
    builder.prefix(prefix);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    let envelope = builder.tempdir_in(target)?.keep();
    let reports = envelope.join("reports");
    fs::create_dir(&reports)?;
    Ok(reports)
}

pub fn run(root: &Path, arguments: &[&str], reports: &Path) -> Result<()> {
    collect(&Client(root), arguments, reports)
}

fn remember(result: &mut Result<()>, next: Result<()>, stage: &'static str) {
    if let Err(error) = next {
        if result.is_ok() {
            *result = Err(error.context(stage));
        } else {
            eprintln!("phase=container_reports stage={stage} error={error:#}");
        }
    }
}

fn collect(docker: &impl Docker, arguments: &[&str], reports: &Path) -> Result<()> {
    let destination = reports.to_str().context("container report path")?;
    let mut create = vec!["create", "--mount", "type=volume,target=/reports"];
    create.extend_from_slice(arguments);
    let id = docker.output(&create)?;
    ensure!(
        id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "invalid report container identity"
    );
    eprintln!("phase=container_reports container={id} reports={destination}");
    let mut result = Ok(());
    remember(&mut result, docker.run(&["start", "--attach", &id]), "run");
    remember(
        &mut result,
        docker.run(&["stop", "--timeout", "10", &id]),
        "stop",
    );
    let status = docker
        .output(&[
            "inspect",
            "--format",
            "{{.State.Status}} {{.State.ExitCode}}",
            &id,
        ])
        .and_then(|state| {
            ensure!(
                state == "exited 0",
                "report container did not succeed: {state}"
            );
            Ok(())
        });
    remember(&mut result, status, "status");
    remember(
        &mut result,
        docker.run(&["cp", &format!("{id}:/reports/."), destination]),
        "collect",
    );
    remember(
        &mut result,
        docker.run(&["rm", "--force", "--volumes", &id]),
        "remove",
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::bail;
    use std::cell::RefCell;

    const ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[derive(Default)]
    struct Effects {
        stopped: bool,
        copied: bool,
        removed: bool,
    }

    struct Fake {
        state: &'static str,
        start_error: bool,
        copy_error: bool,
        remove_error: bool,
        identifier: &'static str,
        effects: RefCell<Effects>,
    }

    impl Fake {
        fn new(state: &'static str) -> Self {
            Self {
                state,
                start_error: false,
                copy_error: false,
                remove_error: false,
                identifier: ID,
                effects: RefCell::new(Effects::default()),
            }
        }
    }

    impl Docker for Fake {
        fn run(&self, arguments: &[&str]) -> Result<()> {
            match arguments {
                ["start", "--attach", ID] if self.start_error => bail!("start rejected"),
                ["start", "--attach", ID] => Ok(()),
                ["stop", "--timeout", "10", ID] => {
                    self.effects.borrow_mut().stopped = true;
                    Ok(())
                }
                ["cp", source, "/private/reports"] => {
                    assert_eq!(*source, format!("{ID}:/reports/."));
                    assert!(self.effects.borrow().stopped);
                    self.effects.borrow_mut().copied = true;
                    if self.copy_error {
                        bail!("copy rejected");
                    }
                    Ok(())
                }
                ["rm", "--force", "--volumes", ID] => {
                    self.effects.borrow_mut().removed = true;
                    if self.remove_error {
                        bail!("removal rejected");
                    }
                    Ok(())
                }
                _ => bail!("unexpected Docker command: {arguments:?}"),
            }
        }

        fn output(&self, arguments: &[&str]) -> Result<String> {
            match arguments {
                ["create", "--mount", "type=volume,target=/reports", ..] => {
                    assert!(
                        !arguments
                            .iter()
                            .any(|argument| argument.contains("type=bind"))
                    );
                    Ok(self.identifier.to_owned())
                }
                [
                    "inspect",
                    "--format",
                    "{{.State.Status}} {{.State.ExitCode}}",
                    ID,
                ] => Ok(self.state.to_owned()),
                _ => bail!("unexpected Docker query: {arguments:?}"),
            }
        }
    }

    #[test]
    fn copies_volume_reports_without_exposing_the_private_host_directory() {
        let docker = Fake::new("exited 0");
        collect(&docker, &["image"], Path::new("/private/reports")).unwrap();
        let effects = docker.effects.borrow();
        assert!(effects.stopped && effects.copied && effects.removed);
    }

    #[test]
    fn failed_or_incomplete_checks_keep_reports_and_release_storage() {
        for state in ["exited 9", "created 0", "running 0", "dead 0", "invalid"] {
            let docker = Fake::new(state);
            assert!(collect(&docker, &["image"], Path::new("/private/reports")).is_err());
            let effects = docker.effects.borrow();
            assert!(effects.copied && effects.removed, "{state}");
        }
    }

    #[test]
    fn collection_and_cleanup_errors_do_not_hide_the_check_failure() {
        let mut docker = Fake::new("exited 9");
        docker.start_error = true;
        docker.copy_error = true;
        docker.remove_error = true;
        let error = collect(&docker, &["image"], Path::new("/private/reports")).unwrap_err();
        assert_eq!(error.root_cause().to_string(), "start rejected");
        assert!(docker.effects.borrow().removed);
        for removal in [false, true] {
            let mut docker = Fake::new("exited 0");
            docker.copy_error = !removal;
            docker.remove_error = removal;
            assert!(collect(&docker, &["image"], Path::new("/private/reports")).is_err());
            assert!(docker.effects.borrow().removed);
        }
    }

    #[test]
    fn invalid_container_identifiers_never_remove_another_container() {
        for identifier in ["", "untrusted", "image:tag"] {
            let mut docker = Fake::new("exited 0");
            docker.identifier = identifier;
            assert!(collect(&docker, &["image"], Path::new("/private/reports")).is_err());
            assert!(!docker.effects.borrow().removed);
        }
    }

    #[test]
    fn report_directories_follow_the_cargo_target_directory() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("checkout");
        let current = directory.path().join("caller");
        let absolute = directory.path().join("cache");
        assert_eq!(parent(&root, &current, None), root.join("target"));
        assert_eq!(
            parent(&root, &current, Some(OsStr::new("cache"))),
            current.join("cache")
        );
        assert_eq!(
            parent(&root, &current, Some(absolute.as_os_str())),
            absolute
        );
    }

    #[cfg(unix)]
    #[test]
    fn copied_directory_permissions_preserve_the_private_parent() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
        let reports = create_directory(&target, "proofs-").unwrap();
        fs::set_permissions(&reports, fs::Permissions::from_mode(0o777)).unwrap();
        let envelope = reports.parent().unwrap();
        assert_eq!(
            fs::metadata(envelope).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}
