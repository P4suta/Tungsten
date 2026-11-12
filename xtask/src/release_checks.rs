use crate::{output, proofs, run};
use anyhow::{Context, Result, ensure};
use clap::ValueEnum;
use serde::Deserialize;
use std::{collections::BTreeSet, fs, path::Path};

const GIB: u64 = 1024 * 1024 * 1024;

fn check_space(available: u64, minimum: u64) -> Result<()> {
    ensure!(
        available >= minimum,
        "native release checks need {minimum} free bytes; {available} are available"
    );
    Ok(())
}

fn require_space(root: &Path, minimum: u64) -> Result<()> {
    let capacity = rustix::fs::statvfs(root)?;
    check_space(capacity.f_bavail.saturating_mul(capacity.f_frsize), minimum)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum NativeCheck {
    Proofs,
    Fuzz,
    Rules,
    LinuxModel,
}

impl NativeCheck {
    fn argument(self) -> &'static str {
        match self {
            Self::Proofs => "proofs",
            Self::Fuzz => "fuzz",
            Self::Rules => "rules",
            Self::LinuxModel => "linux-model",
        }
    }

    fn mutation_target(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Rules => Some(("tungsten", "src/rules.rs")),
            Self::LinuxModel => Some(("tungsten-linux-model", "linux/model/src/state.rs")),
            Self::Proofs | Self::Fuzz => None,
        }
    }

    fn mutation_arguments(self, reports: &Path) -> Result<Vec<String>> {
        let (package, file) = self
            .mutation_target()
            .context("expected a mutation check")?;
        let mut args = [
            "mutants",
            "--in-place",
            "--package",
            package,
            "--file",
            file,
            "--output",
        ]
        .map(str::to_owned)
        .to_vec();
        args.push(reports.to_str().context("mutation report path")?.to_owned());
        if self == Self::Rules {
            args.push("--no-default-features".into());
        }
        args.extend(["--", "--locked"].map(str::to_owned));
        if self == Self::Rules {
            args.extend(["--test", "domain"].map(str::to_owned));
        }
        Ok(args)
    }
}

#[derive(Deserialize)]
struct Mutation {
    name: String,
    package: String,
    file: String,
}

#[derive(Deserialize)]
enum Scenario {
    Baseline,
    Mutant(Mutation),
}

#[derive(Deserialize)]
struct Outcome {
    scenario: Scenario,
    summary: String,
}

#[derive(Deserialize)]
struct Report {
    cargo_mutants_version: String,
    total_mutants: usize,
    caught: usize,
    unviable: usize,
    missed: usize,
    timeout: usize,
    success: usize,
    outcomes: Vec<Outcome>,
}

fn check_mutations(inventory: &[u8], results: &[u8], target: NativeCheck) -> Result<usize> {
    let (package, file) = target
        .mutation_target()
        .context("expected a mutation target")?;
    let inventory: Vec<Mutation> = serde_json::from_slice(inventory)?;
    let report: Report = serde_json::from_slice(results)?;
    let mut expected = BTreeSet::new();
    for mutation in inventory {
        ensure!(
            mutation.package == package
                && mutation.file == file
                && !mutation.name.is_empty()
                && expected.insert(mutation.name),
            "invalid native mutation inventory"
        );
    }
    ensure!(
        !expected.is_empty()
            && report.cargo_mutants_version == "27.1.0"
            && report.total_mutants == expected.len()
            && report.caught > 0
            && report.caught.checked_add(report.unviable) == Some(expected.len())
            && report.missed == 0
            && report.timeout == 0
            && report.success == 0,
        "incomplete or unsuccessful native mutation check"
    );
    let mut executed = BTreeSet::new();
    let mut baselines = 0;
    let mut caught = 0;
    let mut unviable = 0;
    for outcome in report.outcomes {
        match outcome.scenario {
            Scenario::Baseline => {
                ensure!(
                    outcome.summary == "Success",
                    "native mutation baseline failed"
                );
                baselines += 1;
            }
            Scenario::Mutant(mutation) => {
                ensure!(
                    mutation.package == package
                        && mutation.file == file
                        && executed.insert(mutation.name),
                    "invalid or duplicate native mutation outcome"
                );
                match outcome.summary.as_str() {
                    "CaughtMutant" => caught += 1,
                    "Unviable" => unviable += 1,
                    _ => anyhow::bail!("unresolved native mutation outcome"),
                }
            }
        }
    }
    ensure!(
        baselines == 1
            && executed == expected
            && caught == report.caught
            && unviable == report.unviable,
        "native mutation results differ from the inventory"
    );
    Ok(caught)
}

pub fn verify(root: &Path) -> Result<()> {
    require_space(root, 12 * GIB)?;
    proofs::generate(root)?;
    let source = proofs::fingerprint(root)?;
    run(
        root,
        "docker",
        &[
            "build",
            "--platform",
            "linux/amd64",
            "--file",
            "proofs/Dockerfile",
            "--tag",
            "tungsten-release-check:local",
            ".",
        ],
    )?;
    let image = output(
        root,
        "docker",
        &[
            "image",
            "inspect",
            "tungsten-release-check:local",
            "--format",
            "{{.Id}}",
        ],
    )?;
    ensure!(
        image.starts_with("sha256:")
            && image.len() == 71
            && image[7..].bytes().all(|byte| byte.is_ascii_hexdigit()),
        "missing immutable release image identity"
    );
    let reports = tempfile::Builder::new()
        .prefix("release-check-")
        .tempdir_in(root.join("target"))?
        .keep();
    let mount = format!("type=bind,source={},target=/reports", reports.display());
    eprintln!(
        "operation=release-check image={image} reports={}",
        reports.display()
    );
    for check in NativeCheck::value_variants() {
        require_space(root, 8 * GIB)?;
        run(
            root,
            "docker",
            &[
                "run",
                "--rm",
                "--init",
                "--platform",
                "linux/amd64",
                "--memory=4g",
                "--memory-swap=4g",
                "--cpus=2",
                "--pids-limit=256",
                "--cap-drop=ALL",
                "--security-opt=no-new-privileges",
                "--env",
                "CARGO_PROFILE_DEV_DEBUG=0",
                "--env",
                "CARGO_PROFILE_TEST_DEBUG=0",
                "--mount",
                &mount,
                &image,
                "tungsten-xtask",
                "release-native",
                check.argument(),
                "--reports",
                "/reports",
            ],
        )?;
    }
    ensure!(
        proofs::fingerprint(root)? == source,
        "release inputs changed during verification"
    );
    Ok(())
}

pub fn native(root: &Path, selected: Option<NativeCheck>, reports: &Path) -> Result<()> {
    ensure!(
        std::env::consts::OS == "linux",
        "run native release checks in the bounded Linux container"
    );
    for &check in NativeCheck::value_variants() {
        if selected.is_some_and(|selected| selected != check) {
            continue;
        }
        let reports = reports.join(check.argument());
        fs::create_dir_all(&reports)?;
        eprintln!(
            "operation=release-native check={} phase=start",
            check.argument()
        );
        match check {
            NativeCheck::Proofs => proofs::native(root, &reports)?,
            NativeCheck::Fuzz => {
                let prefix = format!("-artifact_prefix={}/", reports.display());
                run(
                    root,
                    "cargo",
                    &[
                        "+nightly-2026-10-01",
                        "fuzz",
                        "run",
                        "rules",
                        "--",
                        "-runs=20000",
                        "-seed=1",
                        "-max_len=512",
                        &prefix,
                    ],
                )?;
            }
            NativeCheck::Rules | NativeCheck::LinuxModel => {
                let args = check.mutation_arguments(&reports)?;
                run(
                    root,
                    "cargo",
                    &args.iter().map(String::as_str).collect::<Vec<_>>(),
                )?;
                let output = reports.join("mutants.out");
                let caught = check_mutations(
                    &fs::read(output.join("mutants.json"))?,
                    &fs::read(output.join("outcomes.json"))?,
                    check,
                )?;
                eprintln!(
                    "operation=rust-mutations check={} caught={caught} outcome=passed",
                    check.argument()
                );
            }
        }
        eprintln!(
            "operation=release-native check={} phase=complete",
            check.argument()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_jobs_require_the_build_and_execution_storage_reserves() {
        assert!(check_space(0, 12 * GIB).is_err());
        assert!(check_space(8 * GIB, 12 * GIB).is_err());
        assert!(check_space(12 * GIB, 12 * GIB).is_ok());
        assert!(check_space(8 * GIB - 1, 8 * GIB).is_err());
        assert!(check_space(8 * GIB, 8 * GIB).is_ok());
    }

    #[test]
    fn mutation_checks_require_complete_nonempty_matching_outcomes() {
        let mutation = json!({"name":"rule", "package":"tungsten", "file":"src/rules.rs"});
        let inventory = serde_json::to_vec(&json!([mutation])).unwrap();
        let valid = json!({"cargo_mutants_version":"27.1.0", "total_mutants":1, "caught":1, "unviable":0, "missed":0, "timeout":0, "success":0, "outcomes":[{"scenario":"Baseline", "summary":"Success"}, {"scenario":{"Mutant":mutation}, "summary":"CaughtMutant"}]});
        let check = |report: &serde_json::Value| {
            check_mutations(
                &inventory,
                &serde_json::to_vec(report).unwrap(),
                NativeCheck::Rules,
            )
        };
        assert_eq!(check(&valid).unwrap(), 1);
        for status in ["MissedMutant", "Timeout", "Success", "Failure"] {
            let mut invalid = valid.clone();
            invalid["outcomes"][1]["summary"] = json!(status);
            assert!(check(&invalid).is_err(), "{status}");
        }
        for pointer in [
            "/cargo_mutants_version",
            "/total_mutants",
            "/caught",
            "/missed",
            "/timeout",
            "/success",
            "/outcomes/1/summary",
            "/outcomes/1/scenario/Mutant/file",
            "/outcomes/1/scenario/Mutant/name",
        ] {
            let mut invalid = valid.clone();
            *invalid.pointer_mut(pointer).unwrap() = match pointer {
                "/total_mutants" | "/missed" | "/timeout" | "/success" => json!(2),
                "/caught" => json!(0),
                _ => json!("invalid"),
            };
            assert!(check(&invalid).is_err(), "{pointer}");
        }
        let mut missing = valid.clone();
        missing["outcomes"].as_array_mut().unwrap().pop();
        assert!(check(&missing).is_err());
        let mut duplicate = valid.clone();
        duplicate["outcomes"]
            .as_array_mut()
            .unwrap()
            .push(valid["outcomes"][1].clone());
        assert!(check(&duplicate).is_err());
        assert!(
            check_mutations(
                b"[]",
                &serde_json::to_vec(&valid).unwrap(),
                NativeCheck::Rules
            )
            .is_err()
        );
    }

    #[test]
    fn native_checks_preserve_the_full_production_scope_and_test_targets() {
        assert_eq!(NativeCheck::value_variants().len(), 4);
        let rules = NativeCheck::Rules
            .mutation_arguments(Path::new("/reports/rules"))
            .unwrap();
        assert!(
            rules
                .windows(2)
                .any(|args| args == ["--file", "src/rules.rs"])
        );
        assert!(rules.iter().any(|arg| arg == "--no-default-features"));
        assert!(rules.ends_with(&["--test".into(), "domain".into()]));
        let model = NativeCheck::LinuxModel
            .mutation_arguments(Path::new("/reports/linux-model"))
            .unwrap();
        assert!(
            model
                .windows(2)
                .any(|args| args == ["--file", "linux/model/src/state.rs"])
        );
        assert!(!model.iter().any(|arg| arg == "--no-default-features"));
        assert!(!model.iter().any(|arg| arg == "--lib" || arg == "--test"));
    }
}
