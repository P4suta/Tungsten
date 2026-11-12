use super::{generate, report};
use crate::{output, run};
use anyhow::{Result, ensure};
use std::{collections::BTreeSet, fs, path::Path};

const SOURCE_INPUTS: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "src/domain.rs",
    "linux/model/src/lib.rs",
    "linux/src/editor.rs",
    "linux/src/drafts.rs",
    "linux/src/client.rs",
    "src/store.rs",
    "src/store/database.rs",
    "src/api.rs",
    "ui/src/model.ts",
    "ui/src/app.ts",
    "ui/src/draft-ownership.ts",
    "proofs/Cargo.toml",
    "proofs/Cargo.lock",
    "proofs/src/lib.rs",
    "proofs/Dockerfile",
    ".dockerignore",
    "xtask/src/proofs.rs",
    "xtask/src/proofs/report.rs",
    "xtask/src/proofs/runner.rs",
];

fn source_inputs() -> impl Iterator<Item = &'static str> {
    SOURCE_INPUTS.iter().copied().chain(
        report::SOURCES
            .iter()
            .flat_map(|source| [source.path, source.production_path]),
    )
}

fn fingerprint(root: &Path) -> Result<tungsten::domain::Revision> {
    let mut inputs = Vec::new();
    for path in source_inputs() {
        let bytes = fs::read(root.join(path))?;
        inputs.push(format!("{path}:{}:", bytes.len()).into_bytes());
        inputs.push(bytes);
    }
    let references = inputs.iter().map(Vec::as_slice).collect::<Vec<_>>();
    Ok(tungsten::domain::Revision::of_parts(&references))
}

fn inventory(root: &Path) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    for source in report::SOURCES {
        for name in report::source_harnesses(&fs::read_to_string(root.join(source.path))?)? {
            ensure!(names.insert(name), "duplicate source harness identity");
        }
    }
    Ok(names)
}

pub fn verify(root: &Path, dry_run: bool) -> Result<()> {
    generate(root)?;
    let expected = inventory(root)?;
    let source = fingerprint(root)?;
    eprintln!(
        "operation=proofs harnesses={} kani=0.68.0 cpus=2 memory=4g swap=0 processes=256 deadline_seconds=890 harness_timeout_seconds=300 sequence_steps=16 estimated_image_gib=6",
        expected.len()
    );
    if dry_run {
        return Ok(());
    }
    run(
        root,
        "docker",
        &[
            "build",
            "--platform",
            "linux/amd64",
            "--file",
            "proofs/Dockerfile",
            "--target",
            "proofs",
            "--tag",
            "tungsten-proofs:local",
            ".",
        ],
    )?;
    let image = output(
        root,
        "docker",
        &[
            "image",
            "inspect",
            "--format",
            "{{.Id}}",
            "tungsten-proofs:local",
        ],
    )?;
    ensure!(
        image.starts_with("sha256:")
            && image.len() == 71
            && image[7..].bytes().all(|byte| byte.is_ascii_hexdigit()),
        "missing immutable proof image identity"
    );
    let reports = tempfile::Builder::new()
        .prefix("proofs-")
        .tempdir_in(root.join("target"))?
        .keep();
    let mount = format!("type=bind,source={},target=/reports", reports.display());
    eprintln!(
        "operation=proofs phase=verify image={image} reports={}",
        reports.display()
    );
    let mut args = vec![
        "run",
        "--rm",
        "--init",
        "--platform",
        "linux/amd64",
        "--network=none",
        "--cpus=2",
        "--memory=4g",
        "--memory-swap=4g",
        "--pids-limit=256",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges",
        "--mount",
        &mount,
        &image,
    ];
    let list_args = [
        "cargo",
        "kani",
        "--manifest-path",
        "/project/proofs/Cargo.toml",
        "list",
        "--format",
        "json",
    ];
    args.extend(list_args);
    run(root, "docker", &args)?;
    args.truncate(args.len() - list_args.len());
    run(root, "docker", &args)?;
    ensure!(
        fingerprint(root)? == source,
        "proof inputs changed during verification; rerun the gate"
    );
    let count = report::check(
        &fs::read(reports.join("kani-list.json"))?,
        &fs::read(reports.join("kani-results.json"))?,
        &expected,
    )?;
    eprintln!("operation=proofs phase=complete harnesses={count}");
    Ok(())
}

pub fn native(root: &Path, reports: &Path) -> Result<()> {
    ensure!(
        std::env::consts::OS == "linux",
        "run proofs in the bounded Linux container"
    );
    generate(root)?;
    let expected = inventory(root)?;
    fs::create_dir_all(reports)?;
    let package = root.join("proofs");
    run(&package, "cargo", &["kani", "list", "--format", "json"])?;
    fs::copy(
        package.join("kani-list.json"),
        reports.join("kani-list.json"),
    )?;
    let result_path = reports.join("kani-results.json");
    run(
        &package,
        "cargo",
        &[
            "kani",
            "-Z",
            "unstable-options",
            "--export-json",
            result_path.to_str().unwrap(),
            "--harness-timeout",
            "300s",
            "--output-format",
            "terse",
        ],
    )?;
    let count = report::check(
        &fs::read(reports.join("kani-list.json"))?,
        &fs::read(result_path)?,
        &expected,
    )?;
    eprintln!("operation=proofs phase=verified harnesses={count}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_source_invalidates_the_verification_identity() {
        let directory = tempfile::tempdir().unwrap();
        for path in source_inputs() {
            let target = directory.path().join(path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, path).unwrap();
        }
        let before = fingerprint(directory.path()).unwrap();
        for path in source_inputs() {
            let target = directory.path().join(path);
            fs::write(&target, format!("{path} changed")).unwrap();
            assert_ne!(fingerprint(directory.path()).unwrap(), before, "{path}");
            fs::write(target, path).unwrap();
        }
        assert_eq!(fingerprint(directory.path()).unwrap(), before);
    }
}
