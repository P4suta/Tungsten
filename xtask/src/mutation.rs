use crate::{output, run};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Deserialize)]
struct Configuration {
    mutate: Vec<String>,
}

#[derive(Deserialize)]
struct Report {
    files: BTreeMap<String, File>,
}

#[derive(Deserialize)]
struct File {
    source: String,
    mutants: Vec<Mutant>,
}

#[derive(Deserialize)]
struct Mutant {
    id: String,
    status: Status,
}

#[derive(Debug, Deserialize)]
enum Status {
    Killed,
    CompileError,
    Survived,
    NoCoverage,
    RuntimeError,
    Timeout,
    Pending,
    Ignored,
}

fn check_report(bytes: &[u8], expected: &BTreeMap<String, String>) -> Result<(usize, usize)> {
    let report: Report = serde_json::from_slice(bytes).context("read the Stryker report")?;
    ensure!(!expected.is_empty(), "mutation scope is empty");
    ensure!(
        report.files.keys().eq(expected.keys()),
        "mutation report doesn't match the configured source inventory"
    );
    let mut killed = 0;
    let mut invalid = 0;
    for (path, file) in report.files {
        ensure!(
            file.source == expected[&path],
            "stale mutation source: {path}"
        );
        ensure!(!file.mutants.is_empty(), "no mutants generated for {path}");
        for mutant in file.mutants {
            match mutant.status {
                Status::Killed => killed += 1,
                Status::CompileError => invalid += 1,
                status => anyhow::bail!("unresolved mutant {} in {path}: {status:?}", mutant.id),
            }
        }
    }
    ensure!(killed > 0, "no executable mutants were checked");
    Ok((killed, invalid))
}

pub fn verify(root: &Path, dry_run: bool) -> Result<()> {
    run(
        root,
        "docker",
        &[
            "build",
            "--file",
            "ui/assurance.Dockerfile",
            "--tag",
            "tungsten-editing-assurance:local",
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
            "tungsten-editing-assurance:local",
        ],
    )?;
    fs::create_dir_all(root.join("target"))?;
    let reports = tempfile::Builder::new()
        .prefix("editing-assurance-")
        .tempdir_in(root.join("target"))?
        .keep();
    eprintln!("operation=editing-assurance reports={}", reports.display());
    let mount = format!(
        "type=bind,source={},target=/project/ui/reports",
        reports.display()
    );
    let mut args = vec![
        "run",
        "--rm",
        "--init",
        "--network=none",
        "--cpus=2",
        "--memory=2g",
        "--memory-swap=2g",
        "--pids-limit=256",
        "--mount",
        &mount,
        &image,
        "npm",
        "run",
        "test:mutations",
    ];
    if dry_run {
        args.extend(["--", "--dryRunOnly"]);
    }
    run(root, "docker", &args)?;
    if !dry_run {
        let configuration: Configuration =
            serde_json::from_slice(&fs::read(root.join("ui/stryker.config.json"))?)?;
        let expected = configuration
            .mutate
            .into_iter()
            .map(|path| {
                let source = fs::read_to_string(root.join("ui").join(&path))?;
                Ok((path, source))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let (killed, invalid) = check_report(&fs::read(reports.join("mutation.json"))?, &expected)?;
        eprintln!(
            "operation=editing-assurance outcome=passed killed={killed} compile_errors={invalid}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn report(status: &str, source: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({ "files": { "src/input.ts": {
            "source": source,
            "mutants": [{"id": "0", "status": "Killed"}, {"id": "1", "status": status}]
        } } }))
        .unwrap()
    }

    #[test]
    fn only_completed_executable_mutations_pass() {
        let expected = BTreeMap::from([("src/input.ts".to_owned(), "source".to_owned())]);
        assert_eq!(
            check_report(&report("Killed", "source"), &expected).unwrap(),
            (2, 0)
        );
        assert_eq!(
            check_report(&report("CompileError", "source"), &expected).unwrap(),
            (1, 1)
        );
        for status in [
            "Survived",
            "NoCoverage",
            "RuntimeError",
            "Timeout",
            "Pending",
            "Ignored",
            "Unknown",
        ] {
            assert!(check_report(&report(status, "source"), &expected).is_err());
        }
    }

    #[test]
    fn stale_or_empty_measurements_fail() {
        let expected = BTreeMap::from([("src/input.ts".to_owned(), "source".to_owned())]);
        assert!(check_report(&report("Killed", "changed"), &expected).is_err());
        assert!(check_report(&report("Killed", "source"), &BTreeMap::new()).is_err());
        assert!(check_report(br#"{"files":{}}"#, &expected).is_err());
        assert!(
            check_report(
                br#"{"files":{"src/input.ts":{"source":"source","mutants":[]}}}"#,
                &expected
            )
            .is_err()
        );
        assert!(check_report(br#"{"files":{"src/input.ts":{"source":"source","mutants":[{"id":"0","status":"CompileError"}]}}}"#, &expected).is_err());
    }
}
