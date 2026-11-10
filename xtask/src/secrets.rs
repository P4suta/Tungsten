use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

#[derive(Clone, Copy, Debug)]
enum Scope {
    Directory,
    History,
}

fn scanner(root: &Path, config: &Path, scope: Scope) -> Result<Command> {
    let args: &[&str] = match scope {
        Scope::Directory => &["dir", "."],
        Scope::History => &["git", ".", "--log-opts=--all"],
    };
    let mut command = crate::verification_command(root, "gitleaks", args)?;
    command
        .arg("--config")
        .arg(config)
        .args(["--redact", "--no-banner"]);
    Ok(command)
}

fn fixture_git(root: &Path, args: &[&str], input: &str) -> Result<String> {
    let mut child = crate::verification_command(root, "git", args)?
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Secret fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Secret fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let written = child
        .stdin
        .take()
        .context("fixture Git input")?
        .write_all(input.as_bytes());
    let result = child.wait_with_output()?;
    written?;
    ensure!(
        result.status.success(),
        "fixture Git failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(String::from_utf8(result.stdout)?.trim().to_owned())
}

#[derive(Deserialize)]
struct Finding {
    #[serde(rename = "File")]
    file: String,
    #[serde(rename = "Commit", default)]
    commit: String,
}

fn require_rejection(root: &Path, config: &Path, scope: Scope, commit: &str) -> Result<()> {
    let result = scanner(root, config, scope)?
        .args(["--report-format=json", "--report-path=-"])
        .output()?;
    let findings: Vec<Finding> = serde_json::from_slice(&result.stdout).with_context(|| {
        format!(
            "fixture secret report: {}",
            String::from_utf8_lossy(&result.stderr)
        )
    })?;
    ensure!(
        result.status.code() == Some(1)
            && findings
                .iter()
                .any(|finding| finding.file == "candidate.txt" && finding.commit == commit),
        "secret scanner did not reject the {scope:?} fixture: expected_commit={commit:?} status={} findings={}",
        result.status,
        findings.len()
    );
    eprintln!("phase=secret_fixture scope={scope:?} outcome=rejected");
    Ok(())
}

fn reject_fixtures(config: &Path) -> Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path();
    fixture_git(root, &["init", "--quiet"], "")?;
    let candidate = ["ghp_", "Q7w8E9r0T1y2U3i4O5p6A7s8D9f0G1h2J3k4"].concat();
    let body = format!("token = {candidate}\n");
    fs::write(root.join("candidate.txt"), &body)?;
    require_rejection(root, config, Scope::Directory, "")?;
    let blob = fixture_git(root, &["hash-object", "-w", "--stdin"], &body)?;
    let tree = fixture_git(
        root,
        &["mktree"],
        &format!("100644 blob {blob}\tcandidate.txt\n"),
    )?;
    let added = fixture_git(root, &["commit-tree", &tree, "-m", "test: add fixture"], "")?;
    let empty = fixture_git(root, &["mktree"], "")?;
    let removed = fixture_git(
        root,
        &[
            "commit-tree",
            &empty,
            "-p",
            &added,
            "-m",
            "test: remove fixture",
        ],
        "",
    )?;
    fixture_git(root, &["update-ref", "refs/heads/main", &removed], "")?;
    fs::remove_file(root.join("candidate.txt"))?;
    require_rejection(root, config, Scope::History, &added)?;
    eprintln!("phase=secrets fixtures=passed");
    Ok(())
}

pub fn verify(root: &Path) -> Result<()> {
    ensure!(
        crate::output(root, "git", &["rev-parse", "--is-shallow-repository"])? == "false",
        "secret checks require complete Git history; run git fetch --unshallow"
    );
    let config = root.join("deploy/gitleaks.toml");
    reject_fixtures(&config)?;
    for scope in [Scope::Directory, Scope::History] {
        eprintln!("phase=secrets scope={scope:?}");
        let status = scanner(root, &config, scope)?.status()?;
        ensure!(
            status.success(),
            "secret scan failed for {scope:?}: {status}"
        );
    }
    Ok(())
}
