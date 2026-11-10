use std::process::Command;

#[test]
fn summaries_can_select_a_single_production_engine() {
    let result = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["storage-summary", "--help"])
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(
        String::from_utf8(result.stdout)
            .unwrap()
            .contains("--engine")
    );
}

#[test]
fn search_selectivity_can_be_planned_without_starting_a_container() {
    let result = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args([
            "storage-profile",
            "--dry-run",
            "--case",
            "search-selectivity",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        String::from_utf8(result.stdout)
            .unwrap()
            .contains("case=SearchSelectivity")
    );
}

#[test]
fn storage_dry_run_plans_every_engine_without_creating_output() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("measurement");
    let result = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args([
            "storage-profile",
            "--dry-run",
            "--engine",
            "all",
            "--case",
            "regression",
            "--output",
        ])
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = String::from_utf8(result.stdout).unwrap();
    for expected in [
        "sqlite",
        "redb",
        "fjall",
        "durability=immediate",
        "memory_mib=2048",
        "allocation_mib=8192",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    assert!(!output.exists());
}

#[test]
fn invalid_storage_workloads_fail_before_starting_containers() {
    for arguments in [
        vec!["--engine", "unknown"],
        vec!["--case", "unknown"],
        vec!["--count", "0"],
        vec!["--note-bytes", "1048577"],
        vec!["--file-bytes", "67108865"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["storage-profile", "--dry-run"])
            .args(arguments)
            .output()
            .unwrap();
        assert!(!result.status.success());
    }
}

#[test]
fn repeated_comparisons_rotate_engines_and_include_a_shared_sqlite_control() {
    let result = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args([
            "storage-profile",
            "--dry-run",
            "--engine",
            "all",
            "--repeats",
            "3",
            "--content",
            "varied",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = String::from_utf8(result.stdout).unwrap();
    let engines = output
        .lines()
        .map(|line| {
            line.split_whitespace()
                .find(|part| part.starts_with("engine="))
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        engines,
        [
            "engine=sqlite",
            "engine=sqlite-kv",
            "engine=redb",
            "engine=fjall",
            "engine=sqlite-kv",
            "engine=redb",
            "engine=fjall",
            "engine=sqlite",
            "engine=redb",
            "engine=fjall",
            "engine=sqlite",
            "engine=sqlite-kv"
        ]
    );
}

#[test]
fn large_fixture_preparation_is_explicit_in_the_plan() {
    let result = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args([
            "storage-profile",
            "--dry-run",
            "--load-fixture",
            "--count",
            "100000",
            "--allocation-gib",
            "4",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        String::from_utf8(result.stdout)
            .unwrap()
            .contains("fixture=bulk")
    );
}

#[test]
fn small_campaigns_accept_mib_allocations_and_reject_conflicting_or_insufficient_limits() {
    let run = |extra: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["storage-profile", "--dry-run", "--count", "64"])
            .args(extra)
            .output()
            .unwrap()
    };
    let result = run(&["--allocation-mib", "512"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        String::from_utf8(result.stdout)
            .unwrap()
            .contains("allocation_mib=512")
    );
    for arguments in [
        vec!["--allocation-mib", "0"],
        vec!["--allocation-mib", "8193"],
        vec!["--allocation-mib", "512", "--allocation-gib", "1"],
        vec!["--allocation-mib", "16", "--note-bytes", "1048576"],
    ] {
        assert!(!run(&arguments).status.success(), "{arguments:?}");
    }
}
