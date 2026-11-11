use super::{Engine, Report, read_report};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

fn collect(path: &Path, reports: &mut Vec<Report>) -> Result<()> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        ensure!(
            !kind.is_symlink(),
            "measurement directories must not contain symlinks"
        );
        if kind.is_dir() {
            collect(&entry.path(), reports)?;
        } else if entry.file_name() == "measurement.json" {
            reports.push(read_report(&entry.path())?);
        } else {
            ensure!(
                entry.file_name() != "failure.txt",
                "campaign contains a failed experiment: {}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

fn distribution(values: impl Iterator<Item = u64>) -> Result<(u64, u64, u64)> {
    let mut values = values.collect::<Vec<_>>();
    values.sort_unstable();
    let first = *values.first().context("missing observations")?;
    let last = *values.last().context("missing observations")?;
    let middle = values.len() / 2;
    let median = if values.len().is_multiple_of(2) {
        ((u128::from(values[middle - 1]) + u128::from(values[middle])) / 2) as u64
    } else {
        values[middle]
    };
    Ok((median, first, last))
}

pub(super) fn run(path: &Path, selected: Engine, repeats: u32) -> Result<()> {
    ensure!((1..=9).contains(&repeats), "invalid expected repetitions");
    let mut reports = Vec::new();
    collect(path, &mut reports)?;
    reports.retain(|report| selected == Engine::All || report.engine == selected);
    ensure!(!reports.is_empty(), "no complete measurements");
    let mut workloads = BTreeMap::<String, Vec<Report>>::new();
    for report in reports {
        workloads
            .entry(serde_json::to_string(&report.workload)?)
            .or_default()
            .push(report);
    }
    for (workload, reports) in workloads {
        let reference = reports.first().context("missing workload")?;
        ensure!(
            reports
                .iter()
                .all(|report| report.source == reference.source
                    && report.environment == reference.environment
                    && report.input_revision == reference.input_revision
                    && report.durability == reference.durability),
            "workload has incompatible source, inputs, or environment"
        );
        for engine in selected.selected() {
            let rows = reports
                .iter()
                .filter(|report| report.engine == engine)
                .collect::<Vec<_>>();
            let trials = rows
                .iter()
                .map(|report| report.replicate)
                .collect::<BTreeSet<_>>();
            ensure!(
                rows.len() == repeats as usize && trials == (1..=repeats).collect(),
                "incomplete or duplicated repetitions for {}: {workload}",
                engine.name()
            );
            let first = rows.first().context("missing engine measurements")?;
            let (rss, rss_min, rss_max) =
                distribution(rows.iter().map(|report| report.process_peak_kib))?;
            let (allocated, allocated_min, allocated_max) =
                distribution(rows.iter().map(|report| report.allocated_bytes))?;
            let (writes, writes_min, writes_max) =
                distribution(rows.iter().map(|report| report.process_write_bytes))?;
            println!(
                "operation=storage_summary engine={} workload={} repeats={} rss_kib_median={} rss_kib_min={} rss_kib_max={} allocated_median={} allocated_min={} allocated_max={} written_median={} written_min={} written_max={}",
                engine.name(),
                workload,
                repeats,
                rss,
                rss_min,
                rss_max,
                allocated,
                allocated_min,
                allocated_max,
                writes,
                writes_min,
                writes_max
            );
            for phase in &first.measurements {
                let phases = rows
                    .iter()
                    .map(|report| {
                        report
                            .measurements
                            .iter()
                            .find(|candidate| candidate.operation == phase.operation)
                            .context("missing repeated phase")
                    })
                    .collect::<Result<Vec<_>>>()?;
                ensure!(
                    phases
                        .iter()
                        .all(|sample| sample.operations == phase.operations
                            && sample.bytes == phase.bytes),
                    "repeated phase inputs differ"
                );
                let (median, min, max) = distribution(phases.iter().map(|phase| phase.p95_us))?;
                let (p50, _, _) = distribution(phases.iter().map(|phase| phase.p50_us))?;
                let (p99, _, _) = distribution(phases.iter().map(|phase| phase.p99_us))?;
                println!(
                    "operation=storage_summary engine={} case={} phase={} count={} content={:?} note_bytes={} file_bytes={} updates={} fixture={} repeats={} samples_per_run={} p50_us_median={} p95_us_median={} p95_us_min={} p95_us_max={} p99_us_median={}",
                    engine.name(),
                    first.workload.case.name(),
                    phase.operation,
                    first.workload.count,
                    first.workload.content,
                    first.workload.note_bytes,
                    first.workload.file_bytes,
                    first.workload.updates,
                    first.workload.load_fixture,
                    repeats,
                    phase.operations,
                    p50,
                    median,
                    min,
                    max,
                    p99
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distributions_preserve_run_variation_and_reject_empty_samples() {
        assert_eq!(distribution([3, 1, 4, 2].into_iter()).unwrap(), (2, 1, 4));
        assert_eq!(
            distribution([90, 10, 20].into_iter()).unwrap(),
            (20, 10, 90)
        );
        assert!(distribution(std::iter::empty()).is_err());
    }
    #[test]
    fn summaries_refuse_missing_runs_and_failure_markers() {
        let directory = tempfile::tempdir().unwrap();
        assert!(run(directory.path(), Engine::All, 4).is_err());
        std::fs::write(directory.path().join("failure.txt"), "incomplete").unwrap();
        assert!(run(directory.path(), Engine::All, 4).is_err());
    }
}
