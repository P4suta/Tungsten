use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct ProofSource {
    pub path: &'static str,
    pub location_suffix: &'static str,
}

pub(super) const SOURCES: &[ProofSource] = &[
    ProofSource {
        path: "src/rules.rs",
        location_suffix: "src/rules.rs",
    },
    ProofSource {
        path: "proofs/src/browser.rs",
        location_suffix: "src/browser.rs",
    },
    ProofSource {
        path: "linux/model/src/state.rs",
        location_suffix: "linux/model/src/state.rs",
    },
];

#[derive(Deserialize)]
struct Inventory {
    #[serde(rename = "kani-version")]
    kani_version: String,
    #[serde(rename = "file-version")]
    file_version: String,
    #[serde(rename = "standard-harnesses")]
    harnesses: BTreeMap<String, Vec<String>>,
}

#[derive(Deserialize)]
struct Metadata {
    kani_version: String,
    version: String,
    target: String,
}

#[derive(Deserialize)]
struct Harness {
    pretty_name: String,
    attributes: Attributes,
}

#[derive(Deserialize)]
struct Attributes {
    kind: String,
    should_panic: bool,
}

#[derive(Deserialize)]
struct Report {
    metadata: Metadata,
    harness_metadata: Vec<Harness>,
    verification_results: Results,
    error_details: Vec<Errors>,
}

#[derive(Deserialize)]
struct Results {
    summary: Summary,
    results: Vec<Outcome>,
}

#[derive(Deserialize)]
struct Summary {
    total_harnesses: usize,
    executed: usize,
    successful: usize,
    failed: usize,
    status: String,
}

#[derive(Deserialize)]
struct Outcome {
    harness_id: String,
    status: String,
    checks: Vec<Check>,
}

#[derive(Deserialize)]
struct Check {
    id: usize,
    status: CheckStatus,
    category: String,
    location: Location,
}

#[derive(Deserialize)]
struct Location {
    file: String,
}

#[derive(Debug, Deserialize, PartialEq)]
enum CheckStatus {
    Success,
    Satisfied,
    Unreachable,
    Failure,
    Covered,
    Undetermined,
    Unknown,
    Uncovered,
    Unsatisfiable,
    Error,
}

#[derive(Deserialize)]
struct Errors {
    harness_id: String,
    has_errors: bool,
}

fn unique(values: impl IntoIterator<Item = String>) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    for value in values {
        ensure!(
            !value.is_empty() && names.insert(value),
            "empty or duplicate proof identity"
        );
    }
    ensure!(!names.is_empty(), "empty proof inventory");
    Ok(names)
}

pub fn source_harnesses(source: &str) -> Result<BTreeSet<String>> {
    fn visit(items: &[syn::Item], names: &mut Vec<String>) -> Result<()> {
        for item in items {
            match item {
                syn::Item::Fn(function)
                    if function.attrs.iter().any(|attr| {
                        attr.path()
                            .segments
                            .iter()
                            .map(|segment| segment.ident.to_string())
                            .eq(["kani", "proof"].map(str::to_owned))
                    }) =>
                {
                    let unwind = function
                        .attrs
                        .iter()
                        .find(|attr| {
                            attr.path()
                                .segments
                                .iter()
                                .map(|segment| segment.ident.to_string())
                                .eq(["kani", "unwind"].map(str::to_owned))
                        })
                        .context("proof has no explicit unwinding bound")?;
                    let bound = unwind.parse_args::<syn::LitInt>()?.base10_parse::<u32>()?;
                    ensure!(bound > 0 && bound <= 4096, "invalid proof unwinding bound");
                    ensure!(
                        !function.attrs.iter().any(|attr| attr
                            .path()
                            .segments
                            .iter()
                            .any(|segment| segment.ident == "should_panic"
                                || segment.ident == "ignore")),
                        "proof harness cannot expect a panic or be ignored"
                    );
                    ensure!(
                        function.sig.inputs.is_empty()
                            && function.sig.generics.params.is_empty()
                            && function.sig.asyncness.is_none()
                            && function.sig.unsafety.is_none(),
                        "unsupported proof harness signature"
                    );
                    names.push(function.sig.ident.to_string());
                }
                syn::Item::Mod(module) => {
                    if let Some((_, items)) = &module.content {
                        visit(items, names)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    let mut names = Vec::new();
    visit(&syn::parse_file(source)?.items, &mut names)?;
    unique(names)
}

fn project_assertion(check: &Check) -> bool {
    check.category == "assertion"
        && SOURCES
            .iter()
            .any(|source| check.location.file.ends_with(source.location_suffix))
}

pub fn check(inventory: &[u8], report: &[u8], expected: &BTreeSet<String>) -> Result<usize> {
    let inventory: Inventory =
        serde_json::from_slice(inventory).context("read Kani harness inventory")?;
    let report: Report =
        serde_json::from_slice(report).context("read Kani verification results")?;
    ensure!(
        inventory.kani_version == "0.68.0" && inventory.file_version == "0.1",
        "unexpected Kani inventory format or tool version"
    );
    ensure!(
        report.metadata.kani_version == "0.68.0"
            && report.metadata.version == "1.0"
            && report.metadata.target == "x86_64-unknown-linux-gnu",
        "unexpected Kani result format, tool version, or target"
    );
    let listed = unique(inventory.harnesses.into_values().flatten())?;
    let short_names = unique(
        listed
            .iter()
            .map(|name| name.rsplit("::").next().unwrap().to_owned()),
    )?;
    ensure!(
        !expected.is_empty() && short_names == *expected,
        "Kani inventory differs from production proof sources"
    );
    let metadata = unique(
        report
            .harness_metadata
            .iter()
            .map(|harness| harness.pretty_name.clone()),
    )?;
    ensure!(
        metadata == listed,
        "Kani result metadata omits or adds a harness"
    );
    ensure!(
        report
            .harness_metadata
            .iter()
            .all(|harness| harness.attributes.kind == "Proof" && !harness.attributes.should_panic),
        "expected a positive proof harness"
    );
    let results = report.verification_results;
    let summary = results.summary;
    ensure!(
        summary.status == "completed"
            && summary.total_harnesses == listed.len()
            && summary.executed == listed.len()
            && summary.successful == listed.len()
            && summary.failed == 0,
        "incomplete or failed Kani verification"
    );
    let executed = unique(
        results
            .results
            .iter()
            .map(|result| result.harness_id.clone()),
    )?;
    ensure!(
        executed == listed,
        "Kani did not execute every listed harness"
    );
    let errors = unique(
        report
            .error_details
            .iter()
            .map(|error| error.harness_id.clone()),
    )?;
    ensure!(
        errors == listed && report.error_details.iter().all(|error| !error.has_errors),
        "Kani reported missing results or tool errors"
    );
    for result in results.results {
        ensure!(
            result.status == "Success" && !result.checks.is_empty(),
            "failed or empty proof: {}",
            result.harness_id
        );
        let mut identifiers = BTreeSet::new();
        let mut assertions = 0usize;
        let mut covers = 0usize;
        for check in &result.checks {
            ensure!(
                identifiers.insert(check.id),
                "duplicate property in {}",
                result.harness_id
            );
            if check.category == "cover" {
                ensure!(
                    check.status == CheckStatus::Satisfied,
                    "unreachable cover in {}",
                    result.harness_id
                );
                covers += 1;
            } else {
                ensure!(
                    matches!(
                        check.status,
                        CheckStatus::Success | CheckStatus::Unreachable
                    ),
                    "inconclusive or failed property in {}: {:?}",
                    result.harness_id,
                    check.status
                );
                if project_assertion(check) {
                    ensure!(
                        check.status == CheckStatus::Success,
                        "unreachable project assertion in {}",
                        result.harness_id
                    );
                    assertions += 1;
                }
            }
        }
        ensure!(
            assertions > 0 && covers > 0,
            "proof lacks reachable assertions or covers: {}",
            result.harness_id
        );
    }
    Ok(listed.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn inventory() -> Value {
        json!({"kani-version":"0.68.0","file-version":"0.1","standard-harnesses":{"src/lib.rs":["example::contract"]}})
    }

    fn report() -> Value {
        json!({
            "metadata":{"kani_version":"0.68.0","version":"1.0","target":"x86_64-unknown-linux-gnu"},
            "harness_metadata":[{"pretty_name":"example::contract","attributes":{"kind":"Proof","should_panic":false}}],
            "verification_results":{
                "summary":{"total_harnesses":1,"executed":1,"successful":1,"failed":0,"status":"completed"},
                "results":[{"harness_id":"example::contract","status":"Success","checks":[
                    {"id":1,"status":"Success","category":"assertion","location":{"file":"proofs/src/browser.rs"}},
                    {"id":2,"status":"Satisfied","category":"cover","location":{"file":"proofs/src/browser.rs"}}
                ]}]
            },
            "error_details":[{"harness_id":"example::contract","has_errors":false}]
        })
    }

    #[test]
    fn proof_reports_reject_missing_failed_vacuous_and_duplicate_results() {
        let expected = BTreeSet::from(["contract".to_owned()]);
        let inventory = serde_json::to_vec(&inventory()).unwrap();
        let valid = report();
        assert_eq!(
            check(&inventory, &serde_json::to_vec(&valid).unwrap(), &expected).unwrap(),
            1
        );
        for path in SOURCES.iter().map(|source| source.location_suffix).chain([
            "src/../../src/rules.rs",
            "src/../../linux/model/src/state.rs",
        ]) {
            let mut relative = valid.clone();
            relative["verification_results"]["results"][0]["checks"][0]["location"]["file"] =
                json!(path);
            assert!(
                check(
                    &inventory,
                    &serde_json::to_vec(&relative).unwrap(),
                    &expected
                )
                .is_ok()
            );
            relative["verification_results"]["results"][0]["checks"][0]["status"] =
                json!("Unreachable");
            assert!(
                check(
                    &inventory,
                    &serde_json::to_vec(&relative).unwrap(),
                    &expected
                )
                .is_err()
            );
        }
        for pointer in [
            "/metadata/kani_version",
            "/harness_metadata/0/attributes/should_panic",
            "/verification_results/summary/executed",
            "/verification_results/results/0/status",
            "/verification_results/results/0/checks/0/status",
            "/verification_results/results/0/checks/1/status",
            "/error_details/0/has_errors",
        ] {
            let mut changed = valid.clone();
            *changed.pointer_mut(pointer).unwrap() = match pointer {
                "/harness_metadata/0/attributes/should_panic" | "/error_details/0/has_errors" => {
                    json!(true)
                }
                "/verification_results/summary/executed" => json!(0),
                _ => json!("Unreachable"),
            };
            assert!(
                check(
                    &inventory,
                    &serde_json::to_vec(&changed).unwrap(),
                    &expected
                )
                .is_err(),
                "{pointer}"
            );
        }
        for field in ["results", "checks", "metadata", "errors"] {
            let mut changed = valid.clone();
            let pointer = match field {
                "results" => "/verification_results/results",
                "checks" => "/verification_results/results/0/checks",
                "metadata" => "/harness_metadata",
                _ => "/error_details",
            };
            let entries = changed
                .pointer_mut(pointer)
                .unwrap()
                .as_array_mut()
                .unwrap();
            entries.push(entries[0].clone());
            assert!(
                check(
                    &inventory,
                    &serde_json::to_vec(&changed).unwrap(),
                    &expected
                )
                .is_err(),
                "{field}"
            );
        }
        assert!(check(&inventory, b"{}", &expected).is_err());
        assert!(
            check(
                &inventory,
                &serde_json::to_vec(&valid).unwrap(),
                &BTreeSet::new()
            )
            .is_err()
        );
        assert!(
            check(
                &inventory,
                &serde_json::to_vec(&valid).unwrap(),
                &BTreeSet::from(["missing".to_owned()])
            )
            .is_err()
        );
    }

    #[test]
    fn source_inventory_requires_bounded_proof_harnesses() {
        let valid = "#[cfg(kani)] mod proofs { #[kani::proof] #[kani::unwind(17)] fn contract() { assert!(true); } }";
        assert_eq!(
            source_harnesses(valid).unwrap(),
            BTreeSet::from(["contract".to_owned()])
        );
        for invalid in [
            valid.replace("#[kani::unwind(17)]", ""),
            valid.replace("#[kani::proof]", "#[kani::proof] #[kani::should_panic]"),
            format!("{valid}\n{valid}"),
        ] {
            assert!(source_harnesses(&invalid).is_err());
        }
    }
}
