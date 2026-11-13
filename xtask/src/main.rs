mod apple;
mod container_reports;
mod demo;
mod linux;
mod memory;
mod mutation;
mod performance;
mod proofs;
mod release_checks;
mod scale;
mod secrets;
mod storage;
mod stress;

use anyhow::{Context, Result, bail, ensure};
use clap::{Parser, Subcommand};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const ADVISORIES: &str = "309392b18f6b17022f82c701f300433126901068";

#[derive(Parser)]
struct Options {
    #[command(subcommand)]
    task: Task,
}
#[derive(Subcommand)]
enum Task {
    Apple {
        #[command(subcommand)]
        action: apple::Action,
    },
    Linux {
        #[command(subcommand)]
        action: linux::Action,
    },
    StorageProfile(storage::Options),
    StorageSummary {
        #[arg(long)]
        input: PathBuf,
        #[arg(long, value_enum, default_value = "all")]
        engine: storage::Engine,
        #[arg(long, default_value_t = 4, value_parser=clap::value_parser!(u32).range(1..=9))]
        repeats: u32,
    },
    #[command(hide = true)]
    StorageProfileNative {
        #[arg(long)]
        request: PathBuf,
    },
    #[command(hide = true)]
    StorageCrashNative {
        #[arg(long, value_enum)]
        engine: storage::Engine,
        #[arg(long)]
        data: PathBuf,
        #[arg(long)]
        commit: bool,
    },
    Setup,
    Build,
    Check,
    Format,
    Docs,
    Dependencies,
    Secrets,
    Proofs {
        #[arg(long)]
        dry_run: bool,
    },
    #[command(hide = true)]
    ProofNative {
        #[arg(long, default_value = "/reports")]
        reports: PathBuf,
    },
    ReleaseCheck,
    #[command(hide = true)]
    ReleaseNative {
        #[arg(value_enum)]
        check: Option<release_checks::NativeCheck>,
        #[arg(long, default_value = "/reports")]
        reports: PathBuf,
    },
    Artifacts,
    EditingAssurance {
        #[arg(long)]
        dry_run: bool,
    },
    BrowserPerformance {
        #[arg(long)]
        dry_run: bool,
    },
    MemoryProfile {
        #[arg(long)]
        dry_run: bool,
    },
    #[command(hide = true)]
    MemoryProfileNative {
        #[arg(long, default_value = "/tungsten")]
        server: PathBuf,
    },
    #[command(hide = true)]
    BrowserPerformanceNative,
    DemoNotes {
        #[arg(long, default_value = "target/demo-notes")]
        output: PathBuf,
        #[arg(long, default_value_t = 10_000)]
        count: usize,
        #[arg(long, default_value_t = 42)]
        seed: u64,
    },
    ScaleProfile {
        #[arg(long)]
        dry_run: bool,
        #[arg(long, value_delimiter = ',', default_value = "10000,100000,1000000")]
        counts: Vec<usize>,
        #[arg(long, default_value_t = 42)]
        seed: u64,
        #[arg(long, value_enum, default_value = "mixed")]
        profile: demo::Profile,
        #[arg(long)]
        reuse_volume: Option<String>,
        #[arg(long)]
        extremes: bool,
        #[arg(long, default_value_t=8, value_parser=clap::value_parser!(u64).range(8..=12))]
        allocation_gib: u64,
    },
    #[command(hide = true)]
    ScaleNative {
        #[arg(long, value_delimiter = ',')]
        counts: Vec<usize>,
        #[arg(long)]
        seed: u64,
        #[arg(long, value_enum)]
        profile: demo::Profile,
        #[arg(long, action=clap::ArgAction::Set,default_value_t=false)]
        reuse_data: bool,
        #[arg(long, action=clap::ArgAction::Set,default_value_t=false)]
        extremes: bool,
        #[arg(long, default_value_t=8, value_parser=clap::value_parser!(u64).range(8..=12))]
        allocation_gib: u64,
    },
}

fn root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_owned)
        .context("workspace root")
}

fn run(directory: &Path, program: &str, args: &[&str]) -> Result<()> {
    run_env(directory, program, args, &[])
}

fn verification_command(directory: &Path, program: &str, args: &[&str]) -> Result<Command> {
    let local = Command::new("git")
        .args(["rev-parse", "--local-env-vars"])
        .output()?;
    ensure!(
        local.status.success(),
        "cannot identify the Git repository environment"
    );
    let mut command = Command::new(program);
    command.args(args).current_dir(directory);
    for name in String::from_utf8(local.stdout)?.lines() {
        command.env_remove(name);
    }
    Ok(command)
}

fn run_env(
    directory: &Path,
    program: &str,
    args: &[&str],
    environment: &[(&str, &str)],
) -> Result<()> {
    eprintln!("phase=command program={program} args={args:?}");
    let status = verification_command(directory, program, args)?
        .envs(environment.iter().copied())
        .stdin(Stdio::null())
        .status()
        .with_context(|| format!("start {program}"))?;
    if matches!(status.code(), Some(124 | 137)) {
        bail!("inconclusive: {program} stopped before verification completed: {status}");
    }
    ensure!(status.success(), "{program} failed: {status}");
    Ok(())
}

fn output(directory: &Path, program: &str, args: &[&str]) -> Result<String> {
    let result = verification_command(directory, program, args)?.output()?;
    ensure!(
        result.status.success(),
        "{program} failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(String::from_utf8(result.stdout)?.trim().to_owned())
}

fn npm(root: &Path, args: &[&str]) -> Result<()> {
    run(&root.join("ui"), "npm", args)
}
fn build_ui(root: &Path) -> Result<()> {
    let chunks = root.join("ui/dist");
    if chunks.exists() {
        fs::remove_dir_all(chunks)?;
    }
    npm(root, &["run", "build"])
}

fn collect(directory: &Path, extension: &str, result: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect(&path, extension, result)?;
        } else if path.extension().is_some_and(|value| value == extension) {
            result.push(path.to_str().context("UTF-8 source path")?.to_owned());
        }
    }
    Ok(())
}

fn docs(root: &Path) -> Result<()> {
    let mut paths = ["README.md", "CONTRIBUTING.md", "SECURITY.md"]
        .into_iter()
        .map(|name| {
            root.join(name)
                .to_str()
                .context("document path")
                .map(str::to_owned)
        })
        .collect::<Result<Vec<_>>>()?;
    collect(&root.join("docs"), "md", &mut paths)?;
    collect(&root.join(".github"), "md", &mut paths)?;
    ensure!(paths.len() >= 4, "missing project documents");
    let shared_source = std::env::var("PROSE_SOURCE").ok();
    let mut args = Vec::new();
    if let Some(source) = shared_source.as_deref() {
        args.extend(["--source", source]);
    }
    args.extend(["check", "--channel", "document"]);
    args.extend(paths.iter().map(String::as_str));
    run(root, "prose", &args)?;
    run(root, "dotfiles", &["repository", "check"])
}

fn dependencies(root: &Path) -> Result<()> {
    run(root, "cargo", &["fetch", "--locked"])?;
    let cache = root.join("target/advisory-db");
    if !cache.exists() {
        run(root, "cargo", &["deny", "fetch"])?;
    }
    let databases = fs::read_dir(&cache)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.join(".git").exists())
        .collect::<Vec<_>>();
    ensure!(
        databases.len() == 1,
        "expected one RustSec database in the project cache"
    );
    let database = &databases[0];
    ensure!(
        output(database, "git", &["remote", "get-url", "origin"])?.trim_end_matches(".git")
            == "https://github.com/RustSec/advisory-db",
        "unexpected advisory source"
    );
    let present = verification_command(database, "git", &["cat-file", "-e", ADVISORIES])?
        .stderr(Stdio::null())
        .status()?;
    if !present.success() {
        run(
            database,
            "git",
            &["fetch", "--depth", "1", "origin", ADVISORIES],
        )?;
    }
    run(database, "git", &["checkout", "--detach", ADVISORIES])?;
    ensure!(
        output(database, "git", &["rev-parse", "HEAD"])? == ADVISORIES,
        "incorrect advisory revision"
    );
    run(
        root,
        "cargo",
        &[
            "deny",
            "--workspace",
            "--all-features",
            "--locked",
            "--offline",
            "check",
        ],
    )?;
    npm(root, &["audit", "--audit-level=high"])?;
    run(
        &root.join("ui/assurance"),
        "npm",
        &["audit", "--package-lock-only", "--audit-level=high"],
    )?;
    secrets::verify(root)
}

fn check(root: &Path) -> Result<()> {
    apple::source(root)?;
    linux::source(root)?;
    run(root, "cargo", &["fmt", "--all", "--check"])?;
    run(
        root,
        "cargo",
        &["fmt", "--manifest-path", "proofs/Cargo.toml", "--check"],
    )?;
    run(
        root,
        "cargo",
        &["fmt", "--manifest-path", "fuzz/Cargo.toml", "--check"],
    )?;
    run(
        root,
        "taplo",
        &[
            "fmt",
            "--check",
            "Cargo.toml",
            "xtask/Cargo.toml",
            "linux/Cargo.toml",
            "linux/model/Cargo.toml",
            "mise.toml",
            ".cargo/config.toml",
            "repository.toml",
            "deny.toml",
            "deploy/gitleaks.toml",
            "proofs/Cargo.toml",
            "fuzz/Cargo.toml",
        ],
    )?;
    run(root, "just", &["--fmt", "--check", "--unstable"])?;
    build_ui(root)?;
    npm(root, &["run", "lint"])?;
    npm(root, &["run", "typecheck"])?;
    npm(root, &["run", "test:unit"])?;
    run(
        root,
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run(
        root,
        "cargo",
        &[
            "clippy",
            "--lib",
            "--no-default-features",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run(root, "cargo", &["test", "--workspace", "--locked"])?;
    proofs::generate(root)?;
    run(
        root,
        "cargo",
        &["check", "--locked", "--manifest-path", "proofs/Cargo.toml"],
    )?;
    run(
        root,
        "cargo",
        &["check", "--locked", "--manifest-path", "fuzz/Cargo.toml"],
    )?;
    run(root, "cargo", &["build", "--locked", "--bin", "tungsten"])?;
    npm(root, &["test"])?;
    run(
        root,
        "hadolint",
        &["Dockerfile", "proofs/Dockerfile", "ui/assurance.Dockerfile"],
    )?;
    run_env(
        root,
        "docker",
        &["compose", "config", "--quiet"],
        &[("TUNGSTEN_ORIGIN", "https://notes.example")],
    )?;
    run(root, "actionlint", &[])?;
    run(
        root,
        "zizmor",
        &[
            "--offline",
            "--persona",
            "pedantic",
            "--strict-collection",
            "--no-config",
            ".github/workflows",
        ],
    )?;
    docs(root)?;
    dependencies(root)
}

fn main() -> Result<()> {
    let root = root()?;
    match Options::parse().task {
        Task::Apple { action } => apple::run(&root, action)?,
        Task::Linux { action } => linux::run(&root, action)?,
        Task::StorageProfile(options) => storage::run(&root, options)?,
        Task::StorageSummary {
            input,
            engine,
            repeats,
        } => storage::summarize(&input, engine, repeats)?,
        Task::StorageProfileNative { request } => storage::native(&request)?,
        Task::StorageCrashNative {
            engine,
            data,
            commit,
        } => storage::crash(engine, &data, commit)?,
        Task::Setup => {
            npm(&root, &["ci"])?;
            npm(
                &root,
                &[
                    "exec",
                    "--",
                    "playwright",
                    "install",
                    "chromium",
                    "firefox",
                    "webkit",
                ],
            )?;
            build_ui(&root)?;
        }
        Task::Build => {
            build_ui(&root)?;
            run(
                &root,
                "cargo",
                &["build", "--release", "--locked", "--bin", "tungsten"],
            )?;
        }
        Task::Check => check(&root)?,
        Task::ScaleProfile {
            dry_run,
            counts,
            seed,
            profile,
            reuse_volume,
            extremes,
            allocation_gib,
        } => scale::run(
            &root,
            &counts,
            seed,
            profile,
            dry_run,
            reuse_volume.as_deref(),
            scale::Workload {
                extremes,
                allocation_gib,
            },
        )?,
        Task::ScaleNative {
            counts,
            seed,
            profile,
            reuse_data,
            extremes,
            allocation_gib,
        } => scale::native(
            &counts,
            seed,
            profile,
            reuse_data,
            scale::Workload {
                extremes,
                allocation_gib,
            },
        )?,
        Task::DemoNotes {
            output,
            count,
            seed,
        } => {
            let report = demo::create(&output, count, seed)?;
            println!(
                "operation=demo_notes phase=complete notes={} links={} seed={seed} entry={}",
                report.notes, report.links, report.entry
            );
        }
        Task::EditingAssurance { dry_run } => mutation::verify(&root, dry_run)?,
        Task::BrowserPerformance { dry_run } => performance::verify(&root, dry_run)?,
        Task::BrowserPerformanceNative => performance::native()?,
        Task::MemoryProfile { dry_run } => memory::verify(&root, dry_run)?,
        Task::MemoryProfileNative { server } => memory::native(&server)?,
        Task::Format => {
            run(&root, "cargo", &["fmt", "--all"])?;
            run(
                &root,
                "taplo",
                &[
                    "fmt",
                    "Cargo.toml",
                    "xtask/Cargo.toml",
                    "linux/Cargo.toml",
                    "linux/model/Cargo.toml",
                    "deny.toml",
                    "repository.toml",
                    "deploy/gitleaks.toml",
                    "proofs/Cargo.toml",
                    "fuzz/Cargo.toml",
                ],
            )?;
            npm(&root, &["exec", "--", "biome", "check", "--write", "."])?;
        }
        Task::Docs => docs(&root)?,
        Task::Dependencies => dependencies(&root)?,
        Task::Secrets => secrets::verify(&root)?,
        Task::Proofs { dry_run } => proofs::verify(&root, dry_run)?,
        Task::ProofNative { reports } => proofs::native(&root, &reports)?,
        Task::ReleaseCheck => {
            check(&root)?;
            mutation::verify(&root, false)?;
            performance::verify(&root, false)?;
            memory::verify(&root, false)?;
            storage::release(&root)?;
            release_checks::verify(&root)?;
        }
        Task::ReleaseNative { check, reports } => {
            release_checks::native(&root, check, &reports)?;
        }
        Task::Artifacts => {
            run(
                &root,
                "docker",
                &[
                    "buildx",
                    "build",
                    "--platform",
                    "linux/amd64,linux/arm64",
                    "--target",
                    "binaries",
                    "--output",
                    "type=local,dest=dist",
                    ".",
                ],
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_use_the_requested_repository_inside_git_hooks() {
        if let Some(directory) = std::env::var_os("TUNGSTEN_CHECK_FOREIGN_REPOSITORY") {
            let directory = PathBuf::from(directory);
            assert_eq!(
                output(&directory, "git", &["config", "test.scope"]).unwrap(),
                "foreign"
            );
            run(&directory, "git", &["config", "test.scope", "checked"]).unwrap();
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        for name in ["parent", "foreign"] {
            run(directory.path(), "git", &["init", "--quiet", name]).unwrap();
            run(
                &directory.path().join(name),
                "git",
                &["config", "test.scope", name],
            )
            .unwrap();
        }
        let parent = directory.path().join("parent");
        let foreign = directory.path().join("foreign");
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::commands_use_the_requested_repository_inside_git_hooks",
            ])
            .env("TUNGSTEN_CHECK_FOREIGN_REPOSITORY", &foreign)
            .env("GIT_DIR", parent.join(".git"))
            .env("GIT_WORK_TREE", &parent)
            .env("GIT_INDEX_FILE", parent.join(".git/index"))
            .env("GIT_COMMON_DIR", parent.join(".git"))
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            output(&parent, "git", &["config", "test.scope"]).unwrap(),
            "parent"
        );
        assert_eq!(
            output(&foreign, "git", &["config", "test.scope"]).unwrap(),
            "checked"
        );
    }

    #[test]
    fn command_failures_are_not_successful_checks() {
        let directory = tempfile::tempdir().unwrap();
        assert!(run(directory.path(), "git", &["not-a-valid-command"]).is_err());
        assert!(run(directory.path(), "tungsten-missing-tool", &[]).is_err());
    }

    #[test]
    fn document_collection_checks_real_markdown_files() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("adr")).unwrap();
        fs::write(directory.path().join("adr/decision.md"), "Decision.").unwrap();
        fs::write(directory.path().join("ignored.txt"), "Ignored.").unwrap();
        let mut paths = Vec::new();
        collect(directory.path(), "md", &mut paths).unwrap();
        assert_eq!(paths.len(), 1);
        assert!(paths[0].ends_with("adr/decision.md"));
    }
}
