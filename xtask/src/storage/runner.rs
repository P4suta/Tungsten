use super::{Case, Content, Engine, Options, workload};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::Command,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tungsten::domain::Revision;
mod summary;

pub(super) fn summarize(path: &Path, engine: Engine, repeats: u32) -> Result<()> {
    summary::run(path, engine, repeats)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Workload {
    case: Case,
    count: u32,
    note_bytes: u32,
    file_bytes: u32,
    updates: u32,
    seed: u64,
    content: Content,
    #[serde(default)]
    load_fixture: bool,
}
impl From<&Options> for Workload {
    fn from(options: &Options) -> Self {
        Self {
            case: options.case,
            count: options.count,
            note_bytes: options.note_bytes,
            file_bytes: options.file_bytes,
            updates: options.updates,
            seed: options.seed,
            content: options.content,
            load_fixture: options.load_fixture,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    options: Options,
    source: Revision,
    commit: String,
    image: String,
    rust: String,
    machine: Revision,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Environment {
    architecture: String,
    kernel: String,
    cpu: String,
    cpu_limit: String,
    memory_limit: String,
    swap_limit: String,
    pids_limit: String,
    rust: String,
    cache: String,
    machine: Revision,
    allocation_sampling_millis: u64,
    data_limit_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Complete,
    Failed,
    ResourceLimit,
}

#[derive(Debug)]
pub(super) struct AllocationLimit {
    allocated: u64,
    limit: u64,
}
impl std::fmt::Display for AllocationLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "storage allocation {} bytes exceeds {} bytes",
            self.allocated, self.limit
        )
    }
}
impl std::error::Error for AllocationLimit {}

impl Environment {
    fn validate(&self) -> Result<()> {
        let cpu = self
            .cpu_limit
            .split_whitespace()
            .map(str::parse::<u64>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ensure!(
            cpu.len() == 2
                && cpu[0] > 0
                && cpu[1] > 0
                && cpu[0] <= cpu[1].checked_mul(2).context("invalid CPU period")?,
            "unbounded storage CPU"
        );
        let memory = self.memory_limit.parse::<u64>()?;
        ensure!(
            memory > 0
                && memory <= 2 * 1024 * 1024 * 1024
                && self.swap_limit.parse::<u64>()? == 0
                && (1..=256).contains(&self.pids_limit.parse::<u64>()?),
            "unbounded storage resources"
        );
        ensure!(
            !self.cpu.is_empty() && !self.rust.is_empty(),
            "missing execution identity"
        );
        ensure!(
            self.allocation_sampling_millis == 250,
            "incompatible allocation sampling"
        );
        ensure!(
            (16 * 1024 * 1024..=8 * 1024 * 1024 * 1024).contains(&self.data_limit_bytes),
            "invalid storage allocation limit"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Measurement {
    operation: String,
    operations: u64,
    p50_us: u64,
    p95_us: u64,
    p99_us: u64,
    max_us: u64,
    total_us: u64,
    bytes: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    format: u32,
    outcome: Outcome,
    engine: Engine,
    replicate: u32,
    workload: Workload,
    source: Revision,
    commit: String,
    image: String,
    environment: Environment,
    durability: String,
    input_revision: Revision,
    measurements: Vec<Measurement>,
    process_peak_kib: u64,
    process_read_bytes: u64,
    process_write_bytes: u64,
    logical_bytes: u64,
    allocated_bytes: u64,
    update_index_entries_changed: Option<u64>,
    failure: Option<String>,
}

fn hash(digest: Sha256) -> Result<Revision> {
    Ok(Revision::from_digest(digest))
}

struct Phase {
    name: String,
    durations: Vec<u64>,
    bytes: u64,
}
pub(super) struct Samples {
    phases: Vec<Phase>,
    inputs: Sha256,
    update_index_entries_changed: u64,
}

impl Samples {
    fn new(options: &Options) -> Result<Self> {
        let mut inputs = Sha256::new();
        inputs.update(serde_json::to_vec(&Workload::from(options))?);
        Ok(Self {
            phases: Vec::new(),
            inputs,
            update_index_entries_changed: 0,
        })
    }
    pub fn input(&mut self, bytes: &[u8]) {
        self.inputs.update((bytes.len() as u64).to_be_bytes());
        self.inputs.update(bytes);
    }
    pub fn index_changes(&mut self, count: usize) -> Result<()> {
        self.update_index_entries_changed = self
            .update_index_entries_changed
            .checked_add(count.try_into()?)
            .context("index mutation count overflow")?;
        Ok(())
    }
    pub fn measure<T>(
        &mut self,
        name: &str,
        bytes: u64,
        operation: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let start = Instant::now();
        let result = operation().with_context(|| format!("storage operation {name} failed"))?;
        let elapsed = u64::try_from(start.elapsed().as_micros())?;
        self.observe(name, bytes, elapsed)?;
        Ok(result)
    }

    pub fn observe(&mut self, name: &str, bytes: u64, elapsed: u64) -> Result<()> {
        let index = match self.phases.iter().position(|phase| phase.name == name) {
            Some(index) => index,
            None => {
                self.phases.push(Phase {
                    name: name.into(),
                    durations: Vec::new(),
                    bytes: 0,
                });
                self.phases.len() - 1
            }
        };
        let phase = self
            .phases
            .get_mut(index)
            .context("missing measurement phase")?;
        phase.bytes = phase
            .bytes
            .checked_add(bytes)
            .context("measurement byte count overflow")?;
        phase.durations.push(elapsed);
        Ok(())
    }
    fn measurements(&self) -> Vec<Measurement> {
        self.phases
            .iter()
            .map(|phase| {
                let mut durations = phase.durations.clone();
                durations.sort_unstable();
                let index = durations
                    .len()
                    .saturating_mul(95)
                    .div_ceil(100)
                    .saturating_sub(1);
                Measurement {
                    operation: phase.name.clone(),
                    operations: durations.len() as u64,
                    p50_us: durations
                        .get(durations.len().div_ceil(2).saturating_sub(1))
                        .copied()
                        .unwrap_or_default(),
                    p95_us: durations.get(index).copied().unwrap_or_default(),
                    p99_us: durations
                        .get(
                            durations
                                .len()
                                .saturating_mul(99)
                                .div_ceil(100)
                                .saturating_sub(1),
                        )
                        .copied()
                        .unwrap_or_default(),
                    max_us: durations.last().copied().unwrap_or_default(),
                    total_us: durations.iter().sum(),
                    bytes: phase.bytes,
                }
            })
            .collect()
    }
}

impl Report {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.format == 2
                && (1..=9).contains(&self.replicate)
                && self.outcome == Outcome::Complete
                && self.failure.is_none()
                && self.engine != Engine::All
                && self.workload.case != Case::All,
            "incomplete storage measurement"
        );
        ensure!(
            self.process_peak_kib > 0
                && self.process_peak_kib <= 2 * 1024 * 1024
                && self.allocated_bytes <= self.environment.data_limit_bytes
                && self.durability == "immediate",
            "invalid measurement environment or allocation"
        );
        self.environment.validate()?;
        let mut names = std::collections::BTreeSet::new();
        for measurement in &self.measurements {
            ensure!(
                names.insert(&measurement.operation)
                    && measurement.operations > 0
                    && measurement.p50_us <= measurement.p95_us
                    && measurement.p95_us <= measurement.p99_us
                    && measurement.p99_us <= measurement.max_us
                    && measurement.max_us <= measurement.total_us,
                "invalid phase measurement"
            );
        }
        let require = |name: &str, count: u64| -> Result<()> {
            let phase = self
                .measurements
                .iter()
                .find(|phase| phase.operation == name)
                .with_context(|| format!("missing measurement phase {name}"))?;
            ensure!(phase.operations == count, "incomplete phase {name}");
            Ok(())
        };
        for name in ["open", "rollback", "reopen"] {
            require(name, 1)?;
        }
        require(
            "create-note",
            if self.workload.load_fixture {
                128
            } else {
                u64::from(self.workload.count)
            },
        )?;
        if self.workload.load_fixture {
            require("fixture-load", 1)?;
            require("fixture-audit", 1)?;
        }
        require(
            "read-note",
            u64::from(self.workload.count).div_ceil(u64::from((self.workload.count / 64).max(1))),
        )?;
        require(
            "search",
            if self.workload.case == Case::Unicode {
                300
            } else {
                200
            },
        )?;
        if self.workload.case == Case::SearchSelectivity {
            for phase in [
                "search-common-positive",
                "search-common-negative",
                "search-window-positive",
                "search-window-negative",
                "search-filtered-negative",
            ] {
                require(phase, 20)?;
            }
        }
        if self.workload.case == Case::ShortNotes && !self.workload.load_fixture {
            require(
                "interleaved-read",
                u64::from(self.workload.count).div_ceil(8),
            )?;
        }
        if matches!(
            self.workload.case,
            Case::Regression | Case::LongEdits | Case::Churn | Case::Recovery
        ) {
            require("save-growing-note", u64::from(self.workload.updates))?;
            if self.engine != Engine::Sqlite {
                require("prepare-update", u64::from(self.workload.updates))?;
                require("commit-update", u64::from(self.workload.updates))?;
            }
        }
        if matches!(
            self.workload.case,
            Case::Regression
                | Case::SmallFiles
                | Case::LargeFiles
                | Case::IncompressibleFiles
                | Case::SharedFiles
                | Case::Recovery
                | Case::Churn
        ) {
            let count = if self.workload.case == Case::SmallFiles {
                u64::from(self.workload.count)
            } else {
                3
            };
            require("upload", count)?;
            let length = if self.workload.case == Case::LargeFiles {
                self.workload.file_bytes.max(16 * 1024 * 1024)
            } else {
                self.workload.file_bytes
            };
            if length > 0 {
                require(
                    "download-range",
                    count * u64::from(length).div_ceil(64 * 1024),
                )?;
            }
        }
        if matches!(
            self.workload.case,
            Case::Regression | Case::Recovery | Case::Churn
        ) {
            for name in ["create-workspace", "pin-note", "workspace-membership"] {
                require(name, 1)?;
            }
        }
        if self.workload.case == Case::Churn {
            require("delete-note", u64::from(self.workload.updates))?;
            require("recreate-note", u64::from(self.workload.updates))?;
        }
        if self.workload.case == Case::Recovery {
            for name in [
                "retained-file-cleanup",
                "expired-recovery-cleanup",
                "unreferenced-file-cleanup",
            ] {
                require(name, 1)?;
            }
        }
        if matches!(self.workload.case, Case::Regression | Case::Crash) {
            require("crash-reopen", 2)?;
        }
        if self.workload.case == Case::SharedFiles {
            require("shared-reference", u64::from(self.workload.count))?;
        }
        if self.workload.case == Case::Hub && self.workload.note_bytes >= 128 {
            require("incoming-links", 1)?;
        }
        let creates = self
            .measurements
            .iter()
            .find(|phase| phase.operation == "create-note")
            .context("missing creation measurements")?;
        ensure!(
            creates.operations
                == if self.workload.load_fixture {
                    128
                } else {
                    u64::from(self.workload.count)
                },
            "incomplete creation workload"
        );
        if matches!(
            self.workload.case,
            Case::Regression | Case::Recovery | Case::Churn
        ) {
            for required in ["complete-backup", "complete-restore", "portable-restore"] {
                ensure!(
                    names.contains(&required.to_owned()),
                    "missing recovery phase {required}"
                );
            }
        }
        Ok(())
    }

    fn compare(&self, baseline: &Self) -> Result<()> {
        self.validate()?;
        baseline.validate()?;
        ensure!(
            self.workload == baseline.workload && self.input_revision == baseline.input_revision,
            "baseline workload or generated input differs"
        );
        ensure!(
            self.environment == baseline.environment && self.durability == baseline.durability,
            "baseline environment or durability differs"
        );
        for phase in &self.measurements {
            if matches!(phase.operation.as_str(), "prepare-update" | "commit-update")
                && baseline.engine == Engine::Sqlite
            {
                continue;
            }
            let previous = baseline
                .measurements
                .iter()
                .find(|previous| previous.operation == phase.operation)
                .with_context(|| format!("baseline lacks phase {}", phase.operation))?;
            ensure!(
                phase.operations == previous.operations && phase.bytes == previous.bytes,
                "baseline operation counts differ"
            );
            println!(
                "operation=storage_compare phase={} baseline_engine={} engine={} baseline_p95_us={} p95_us={} baseline_max_us={} max_us={}",
                phase.operation,
                baseline.engine.name(),
                self.engine.name(),
                previous.p95_us,
                phase.p95_us,
                previous.max_us,
                phase.max_us
            );
        }
        ensure!(
            self.measurements
                .iter()
                .filter(|phase| !matches!(
                    phase.operation.as_str(),
                    "prepare-update" | "commit-update"
                ))
                .count()
                == baseline
                    .measurements
                    .iter()
                    .filter(|phase| !matches!(
                        phase.operation.as_str(),
                        "prepare-update" | "commit-update"
                    ))
                    .count(),
            "baseline phase sets differ"
        );
        Ok(())
    }
}

fn read_report(path: &Path) -> Result<Report> {
    let file = File::open(path)?;
    ensure!(
        file.metadata()?.len() <= 1024 * 1024,
        "measurement report is too large"
    );
    let report: Report = serde_json::from_reader(file)?;
    report.validate()?;
    Ok(report)
}

fn source_identity(root: &Path) -> Result<(Revision, String)> {
    let commit = crate::output(root, "git", &["rev-parse", "HEAD"])?;
    let tracked = crate::output(root, "git", &["ls-files", "-z"])?;
    let untracked = crate::output(
        root,
        "git",
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?;
    let mut paths = tracked
        .split('\0')
        .chain(untracked.split('\0'))
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    paths.sort_unstable();
    paths.dedup();
    let mut digest = Sha256::new();
    for name in paths {
        let path = root.join(name);
        let metadata = match path.symlink_metadata() {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                digest.update(b"deleted");
                digest.update(name);
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        digest.update(name);
        digest.update([0]);
        if metadata.file_type().is_symlink() {
            digest.update(fs::read_link(path)?.as_os_str().as_encoded_bytes());
        } else if metadata.is_file() {
            let mut file = File::open(path)?;
            let mut buffer = [0; 64 * 1024];
            loop {
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                digest.update(&buffer[..count]);
            }
        }
    }
    Ok((hash(digest)?, commit.trim().into()))
}

pub(super) fn run(root: &Path, options: Options) -> Result<()> {
    options.validate()?;
    if let Some(baseline) = &options.baseline {
        let report = read_report(baseline)?;
        ensure!(
            report.workload == Workload::from(&options),
            "baseline workload differs"
        );
    }
    for selected in options.experiments() {
        println!(
            "operation=storage_profile phase=plan engine={} case={:?} count={} seed={} replicate={} content={:?} fixture={} durability=immediate cpus=2 memory_mib=2048 swap_mib=0 pids=256 deadline_seconds=900 allocation_mib={} campaign_seconds={} cache=new-process-uncontrolled-os-cache",
            selected.engine.name(),
            selected.case,
            options.count,
            options.seed,
            selected.replicate,
            selected.content,
            if selected.load_fixture {
                "bulk"
            } else {
                "individual"
            },
            options.allocation_bytes() / (1024 * 1024),
            options.max_seconds
        );
    }
    if options.dry_run {
        return Ok(());
    }
    let (source, commit) = source_identity(root)?;
    let rust = crate::output(root, "rustc", &["--version"])?;
    let path = match &options.output {
        Some(path) => path.clone(),
        None => root.join("target").join(format!(
            "storage-profile-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        )),
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(&path).context("measurement output must be unused")?;
    crate::run(
        root,
        "docker",
        &[
            "build",
            "--target",
            "storage-profile",
            "--tag",
            "tungsten-storage-profile:local",
            ".",
        ],
    )?;
    let image = crate::output(
        root,
        "docker",
        &[
            "image",
            "inspect",
            "tungsten-storage-profile:local",
            "--format",
            "{{.Id}}",
        ],
    )?;
    ensure!(
        image.trim().starts_with("sha256:"),
        "missing storage image identity"
    );
    ensure!(
        source_identity(root)?.0 == source,
        "source changed while building the storage image"
    );
    let machine_id = crate::output(root, "docker", &["info", "--format", "{{.ID}}"])?;
    ensure!(
        !machine_id.trim().is_empty(),
        "missing Docker daemon identity"
    );
    let machine = Revision::of(machine_id.trim().as_bytes());
    let mut failures = Vec::new();
    let started = Instant::now();
    for mut selected in options.experiments() {
        if started.elapsed().as_secs().saturating_add(900) > options.max_seconds {
            failures.push("campaign time budget exhausted before all experiments started".into());
            break;
        }
        let engine = selected.engine;
        let path = if options.repeats > 1 {
            let repeated = path.join(format!("replicate-{}", selected.replicate));
            fs::create_dir_all(&repeated)?;
            repeated
        } else {
            path.clone()
        };
        let output = if options.case == Case::All {
            let parent = path.join(engine.name());
            fs::create_dir_all(&parent)?;
            parent.join(selected.case.name())
        } else {
            path.join(engine.name())
        };
        fs::create_dir(&output)?;
        selected.output = None;
        selected.baseline = options
            .baseline
            .as_ref()
            .map(|_| PathBuf::from("/baseline.json"));
        let request = Request {
            options: selected,
            source: source.clone(),
            commit: commit.clone(),
            image: image.trim().into(),
            rust: rust.trim().into(),
            machine: machine.clone(),
        };
        let mut request_file = File::create(output.join("request.json"))?;
        serde_json::to_writer(&mut request_file, &request)?;
        request_file.sync_all()?;
        let output = output.canonicalize()?;
        ensure!(
            !output.as_os_str().as_encoded_bytes().contains(&b','),
            "Docker output paths can't contain commas"
        );
        let mount = format!(
            "type=bind,source={},target=/request.json,readonly",
            output.join("request.json").display()
        );
        let mut command = Command::new("docker");
        command.current_dir(root).args([
            "run",
            "--rm",
            "--init",
            "--network=none",
            "--read-only",
            "--cpus=2",
            "--memory=2g",
            "--memory-swap=2g",
            "--pids-limit=256",
            "--tmpfs",
            "/tmp:rw,nosuid,nodev,size=33554432",
            "--mount",
            "type=volume,target=/data",
            "--mount",
            &mount,
        ]);
        if let Some(baseline) = &options.baseline {
            let baseline = baseline.canonicalize()?;
            ensure!(
                !baseline.as_os_str().as_encoded_bytes().contains(&b','),
                "Docker baseline paths can't contain commas"
            );
            command.args([
                "--mount",
                &format!(
                    "type=bind,source={},target=/baseline.json,readonly",
                    baseline.display()
                ),
            ]);
        }
        let result = command.arg(image.trim()).output()?;
        fs::write(output.join("stderr.log"), &result.stderr)?;
        eprint!("{}", String::from_utf8_lossy(&result.stderr));
        let stdout = String::from_utf8(result.stdout)?;
        let mut record_outcome = None;
        let mut reports = stdout
            .lines()
            .filter_map(|line| line.strip_prefix("storage_report="));
        if let Some(record) = reports.next() {
            ensure!(
                reports.next().is_none(),
                "duplicate storage measurement reports"
            );
            let report: Report = serde_json::from_str(record)?;
            record_outcome = Some(report.outcome);
            let mut file = File::options()
                .write(true)
                .create_new(true)
                .open(output.join("measurement.json"))?;
            serde_json::to_writer_pretty(&mut file, &report)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
        }
        for line in stdout
            .lines()
            .filter(|line| !line.starts_with("storage_report="))
        {
            println!("{line}");
        }
        let status = result.status;
        if !status.success() {
            let outcome =
                if status.code() == Some(75) || record_outcome == Some(Outcome::ResourceLimit) {
                    "resource_limit"
                } else if record_outcome == Some(Outcome::Failed) {
                    "failed"
                } else {
                    "incomplete"
                };
            fs::write(
                output.join("failure.txt"),
                format!("outcome={outcome} exit={status}\n{stdout}"),
            )?;
            failures.push(format!(
                "engine={} case={} exit={status} output={}",
                engine.name(),
                request.options.case.name(),
                output.display()
            ));
            continue;
        }
        ensure!(
            status.success(),
            "storage measurement for {} is incomplete: {status}; retained output: {}",
            engine.name(),
            output.display()
        );
        read_report(&output.join("measurement.json"))?;
        println!(
            "operation=storage_profile outcome=complete engine={} output={}",
            engine.name(),
            output.display()
        );
    }
    ensure!(
        failures.is_empty(),
        "incomplete storage experiments:\n{}",
        failures.join("\n")
    );
    Ok(())
}

fn field(path: &str, name: &str) -> Result<u64> {
    let text = fs::read_to_string(path)?;
    let mut values = text.lines().filter_map(|line| line.strip_prefix(name));
    let value = values
        .next()
        .with_context(|| format!("missing process field {name}"))?
        .split_whitespace()
        .next()
        .context("empty process field")?
        .parse()?;
    ensure!(values.next().is_none(), "duplicate process field");
    Ok(value)
}

fn sizes(path: &Path) -> Result<(u64, u64)> {
    let mut logical = 0u64;
    let mut allocated = 0u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = match entry.path().symlink_metadata() {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        ensure!(
            !metadata.file_type().is_symlink(),
            "unexpected data symlink"
        );
        let (bytes, blocks) = if metadata.is_dir() {
            match sizes(&entry.path()) {
                Ok(value) => value,
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
                {
                    (0, 0)
                }
                Err(error) => return Err(error),
            }
        } else {
            (metadata.len(), metadata.blocks() * 512)
        };
        logical = logical
            .checked_add(bytes)
            .context("logical size overflow")?;
        allocated = allocated
            .checked_add(blocks)
            .context("allocation size overflow")?;
    }
    Ok((logical, allocated))
}

pub(super) fn check_budget(path: &Path, limit: u64) -> Result<u64> {
    let allocated = sizes(path)?.1;
    if allocated > limit {
        return Err(AllocationLimit { allocated, limit }.into());
    }
    Ok(allocated)
}

pub(super) fn check_allocation(path: &Path, limit: u64) -> Result<()> {
    check_budget(path, limit)?;
    Ok(())
}

pub(super) fn native(path: &Path) -> Result<()> {
    ensure!(
        std::env::consts::OS == "linux",
        "run storage measurements in their bounded Linux container"
    );
    let request: Request = serde_json::from_reader(File::open(path)?)?;
    request.options.validate()?;
    ensure!(
        request.options.engine != Engine::All && request.options.case != Case::All,
        "native measurements require one engine"
    );
    let cpu_limit = fs::read_to_string("/sys/fs/cgroup/cpu.max")?
        .trim()
        .to_owned();
    let memory_limit = fs::read_to_string("/sys/fs/cgroup/memory.max")?
        .trim()
        .to_owned();
    ensure!(
        memory_limit.parse::<u64>()? <= 2 * 1024 * 1024 * 1024,
        "unbounded storage memory"
    );
    let cpu = cpu_limit
        .split_whitespace()
        .map(str::parse::<u64>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(
        cpu.len() == 2 && cpu[0] <= cpu[1].checked_mul(2).context("invalid CPU period")?,
        "unbounded storage CPU"
    );
    let cpu_info = fs::read_to_string("/proc/cpuinfo")?;
    let environment = Environment {
        architecture: std::env::consts::ARCH.into(),
        kernel: fs::read_to_string("/proc/sys/kernel/osrelease")?
            .trim()
            .into(),
        cpu: cpu_info
            .lines()
            .filter(|line| {
                [
                    "model name",
                    "Hardware",
                    "CPU implementer",
                    "CPU architecture",
                    "CPU part",
                    "CPU variant",
                    "CPU revision",
                ]
                .iter()
                .any(|prefix| line.starts_with(prefix))
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join("\n"),
        cpu_limit,
        memory_limit,
        swap_limit: fs::read_to_string("/sys/fs/cgroup/memory.swap.max")?
            .trim()
            .into(),
        pids_limit: fs::read_to_string("/sys/fs/cgroup/pids.max")?.trim().into(),
        rust: request.rust,
        cache: "new-process-uncontrolled-os-cache".into(),
        machine: request.machine,
        allocation_sampling_millis: 250,
        data_limit_bytes: request.options.allocation_bytes(),
    };
    environment.validate()?;
    let capacity = rustix::fs::statvfs("/data")?;
    let available = capacity.f_bavail.saturating_mul(capacity.f_frsize);
    ensure!(
        available >= environment.data_limit_bytes + 1024 * 1024 * 1024,
        "insufficient free space for the campaign and its reserve"
    );
    let allocation = super::allocation::AllocationWatch::start(
        Path::new("/data"),
        environment.data_limit_bytes,
        |error| {
            eprintln!("operation=allocation_watch outcome=failed error={error:#}");
            std::process::exit(if error.downcast_ref::<AllocationLimit>().is_some() {
                75
            } else {
                76
            });
        },
    )?;
    let mut samples = Samples::new(&request.options)?;
    let outcome = workload::execute(
        &request.options,
        request.options.engine,
        Path::new("/data"),
        &mut samples,
    );
    let allocation_peak = allocation.finish()?;
    let (logical_bytes, allocated_bytes) = sizes(Path::new("/data"))?;
    let allocated_bytes = allocated_bytes.max(allocation_peak);
    let mut report = Report {
        format: 2,
        outcome: if outcome
            .as_ref()
            .err()
            .is_some_and(|error| error.downcast_ref::<AllocationLimit>().is_some())
        {
            Outcome::ResourceLimit
        } else if outcome.is_ok() {
            Outcome::Complete
        } else {
            Outcome::Failed
        },
        engine: request.options.engine,
        replicate: request.options.replicate,
        workload: Workload::from(&request.options),
        source: request.source,
        commit: request.commit,
        image: request.image,
        environment,
        durability: "immediate".into(),
        input_revision: hash(samples.inputs.clone())?,
        measurements: samples.measurements(),
        process_peak_kib: field("/proc/self/status", "VmHWM:")?,
        process_read_bytes: field("/proc/self/io", "read_bytes:")?,
        process_write_bytes: field("/proc/self/io", "write_bytes:")?,
        logical_bytes,
        allocated_bytes,
        update_index_entries_changed: (request.options.engine != Engine::Sqlite)
            .then_some(samples.update_index_entries_changed),
        failure: outcome.as_ref().err().map(|error| format!("{error:#}")),
    };
    if report.outcome == Outcome::Complete {
        report.validate()?;
    }
    let comparison = if outcome.is_ok() {
        request
            .options
            .baseline
            .as_ref()
            .map(|path| read_report(path).and_then(|baseline| report.compare(&baseline)))
            .transpose()
    } else {
        Ok(None)
    };
    if let Err(error) = &comparison {
        report.outcome = Outcome::Failed;
        report.failure = Some(format!("{error:#}"));
    }
    println!("storage_report={}", serde_json::to_string(&report)?);
    outcome?;
    comparison?;
    for phase in &report.measurements {
        println!(
            "operation=storage_measure phase={} operations={} bytes={} p95_us={} max_us={} total_us={}",
            phase.operation,
            phase.operations,
            phase.bytes,
            phase.p95_us,
            phase.max_us,
            phase.total_us
        );
    }
    println!(
        "operation=storage_measure outcome=complete engine={} process_peak_kib={} process_write_bytes={} logical_bytes={} allocated_bytes={}",
        report.engine.name(),
        report.process_peak_kib,
        report.process_write_bytes,
        report.logical_bytes,
        report.allocated_bytes
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> Report {
        Report {
            format: 2,
            outcome: Outcome::Complete,
            engine: Engine::Sqlite,
            replicate: 1,
            workload: Workload {
                case: Case::ShortNotes,
                count: 2,
                note_bytes: 1,
                file_bytes: 0,
                updates: 1,
                seed: 42,
                content: Content::Repeated,
                load_fixture: false,
            },
            source: Revision::of(b"source"),
            commit: "source-commit".into(),
            image: "sha256:image".into(),
            environment: Environment {
                architecture: "aarch64".into(),
                kernel: "kernel".into(),
                cpu: "cpu".into(),
                cpu_limit: "200000 100000".into(),
                memory_limit: "2147483648".into(),
                swap_limit: "0".into(),
                pids_limit: "256".into(),
                rust: "rustc".into(),
                cache: "new-process-uncontrolled-os-cache".into(),
                machine: Revision::of(b"machine"),
                allocation_sampling_millis: 250,
                data_limit_bytes: 8 * 1024 * 1024 * 1024,
            },
            durability: "immediate".into(),
            input_revision: Revision::of(b"input"),
            measurements: [
                ("open", 1),
                ("create-note", 2),
                ("read-note", 2),
                ("search", 200),
                ("rollback", 1),
                ("reopen", 1),
                ("interleaved-read", 1),
            ]
            .into_iter()
            .map(|(name, count)| Measurement {
                operation: name.into(),
                operations: count,
                p50_us: 1,
                p95_us: 1,
                p99_us: 2,
                max_us: 2,
                total_us: count * 2,
                bytes: 0,
            })
            .collect(),
            process_peak_kib: 1000,
            process_read_bytes: 0,
            process_write_bytes: 4096,
            logical_bytes: 4096,
            allocated_bytes: 4096,
            update_index_entries_changed: None,
            failure: None,
        }
    }

    #[test]
    fn selectivity_comparisons_require_every_verified_query_group() {
        let mut complete = report();
        complete.workload.case = Case::SearchSelectivity;
        let mut phase = complete.measurements[0].clone();
        phase.operations = 20;
        phase.total_us = 40;
        for name in [
            "search-common-positive",
            "search-common-negative",
            "search-window-positive",
            "search-window-negative",
            "search-filtered-negative",
        ] {
            let mut item = phase.clone();
            item.operation = name.into();
            complete.measurements.push(item);
        }
        assert!(complete.validate().is_ok());
        for name in [
            "search-common-positive",
            "search-common-negative",
            "search-window-positive",
            "search-window-negative",
            "search-filtered-negative",
        ] {
            let index = complete
                .measurements
                .iter()
                .position(|phase| phase.operation == name)
                .unwrap();
            let removed = complete.measurements.remove(index);
            assert!(complete.validate().is_err(), "{name}");
            complete.measurements.insert(index, removed);
        }
    }

    #[test]
    fn incomplete_incompatible_and_over_budget_measurements_are_refused() {
        assert!(report().validate().is_ok());
        for index in 0..report().measurements.len() {
            let mut incomplete = report();
            incomplete.measurements.remove(index);
            assert!(incomplete.validate().is_err(), "missing phase {index}");
        }
        let mut incomplete = report();
        incomplete
            .measurements
            .iter_mut()
            .find(|phase| phase.operation == "search")
            .unwrap()
            .operations -= 1;
        assert!(incomplete.validate().is_err());
        let mut exceeded = report();
        exceeded.process_peak_kib = 2 * 1024 * 1024 + 1;
        assert!(exceeded.validate().is_err());
        let mut exceeded = report();
        exceeded.allocated_bytes = 8 * 1024 * 1024 * 1024 + 1;
        assert!(exceeded.validate().is_err());
        let mut failed = report();
        failed.outcome = Outcome::Failed;
        assert!(failed.validate().is_err());
        let mut other = report();
        other.workload.seed += 1;
        assert!(report().compare(&other).is_err());
        let mut other = report();
        other.environment.machine = Revision::of(b"other-machine");
        assert!(report().compare(&other).is_err());
        let mut other = report();
        other.input_revision = Revision::of(b"changed input");
        assert!(report().compare(&other).is_err());
        let mut other = report();
        other.engine = Engine::Redb;
        assert!(report().compare(&other).is_ok());
    }
    #[test]
    fn phase_statistics_keep_tail_observations_and_byte_counts() {
        let samples = Samples {
            phases: vec![Phase {
                name: "read".into(),
                durations: (1..=100).collect(),
                bytes: 250,
            }],
            inputs: Sha256::new(),
            update_index_entries_changed: 0,
        };
        let phases = samples.measurements();
        assert_eq!(
            (
                phases[0].operations,
                phases[0].p95_us,
                phases[0].max_us,
                phases[0].total_us,
                phases[0].bytes
            ),
            (100, 95, 100, 5050, 250)
        );
    }
}
