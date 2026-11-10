use crate::demo::{Corpus, Profile};
use anyhow::{Context, Result, bail, ensure};
use pulldown_cmark::{Event, Parser, Tag};
use std::{
    collections::HashSet,
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const BODY_BUDGET: usize = 4 * 1024 * 1024;
const REQUEST_SECONDS: u64 = 15;

pub(super) struct Workload {
    pub(super) extremes: bool,
    pub(super) allocation_gib: u64,
}

impl Workload {
    fn validate(&self) -> Result<()> {
        ensure!(
            (8..=12).contains(&self.allocation_gib),
            "allocation must remain between 8 and 12 GiB"
        );
        Ok(())
    }
}

fn plan(counts: &[usize]) -> Result<()> {
    ensure!(!counts.is_empty(), "select at least one dataset size");
    ensure!(
        counts.iter().all(|count| (420..=1_000_000).contains(count)),
        "dataset sizes must be between 420 and 1000000"
    );
    ensure!(
        counts.windows(2).all(|pair| pair[0] < pair[1]),
        "dataset sizes must increase without duplicates"
    );
    ensure!(
        counts.iter().sum::<usize>() <= 1_200_000,
        "one campaign can contain at most 1200000 total pages"
    );
    Ok(())
}

#[derive(Debug, PartialEq)]
enum Outcome {
    Complete,
    HttpFailure,
    ResponseBudget,
}

impl Outcome {
    fn name(&self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::HttpFailure => "http_failure",
            Self::ResponseBudget => "response_budget_exceeded",
        }
    }
}

fn classify(status: u16, length: usize) -> Outcome {
    if !(200..300).contains(&status) {
        Outcome::HttpFailure
    } else if length > BODY_BUDGET {
        Outcome::ResponseBudget
    } else {
        Outcome::Complete
    }
}

fn headers(status: &str, fields: &[String]) -> Result<(u16, usize)> {
    let mut status = status.split_whitespace();
    ensure!(
        status.next() == Some("HTTP/1.1"),
        "unexpected local HTTP protocol"
    );
    let code = status.next().context("missing HTTP status")?.parse()?;
    let mut length = None;
    for field in fields {
        if let Some((name, value)) = field.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                ensure!(length.is_none(), "duplicate local content length");
                length = Some(value.trim().parse()?);
            }
            if name.eq_ignore_ascii_case("transfer-encoding") {
                bail!("unexpected streaming response from local JSON endpoint");
            }
        }
    }
    let length = match (code, length) {
        (204, None) => 0,
        (_, Some(length)) => length,
        _ => bail!("missing local content length"),
    };
    Ok((code, length))
}

fn request(path: &str, deadline: Instant) -> Result<(Outcome, u16, usize, Vec<u8>)> {
    let address = SocketAddr::from(([127, 0, 0, 1], 18081));
    request_at(address, path, deadline)
}

fn request_at(
    address: SocketAddr,
    path: &str,
    deadline: Instant,
) -> Result<(Outcome, u16, usize, Vec<u8>)> {
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(1))?;
    let remaining = || {
        deadline
            .checked_duration_since(Instant::now())
            .context("request deadline exceeded")
    };
    stream.set_write_timeout(Some(remaining()?))?;
    let mut writer = &stream;
    write!(
        writer,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:18081\r\nConnection: close\r\n\r\n"
    )?;
    let mut reader = BufReader::new(stream);
    let mut lines = Vec::new();
    let mut total = 0;
    loop {
        reader.get_ref().set_read_timeout(Some(remaining()?))?;
        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        ensure!(bytes != 0, "local response ended before its headers");
        total += bytes;
        ensure!(total <= 16 * 1024, "local response headers exceeded 16 KiB");
        if line == "\r\n" {
            break;
        }
        lines.push(line);
    }
    let (status, length) = headers(lines.first().context("missing status")?, &lines[1..])?;
    let outcome = classify(status, length);
    let mut body = Vec::new();
    if outcome == Outcome::Complete {
        let mut remaining_bytes = length;
        let mut buffer = [0; 8192];
        while remaining_bytes > 0 {
            reader.get_ref().set_read_timeout(Some(remaining()?))?;
            let chunk = remaining_bytes.min(buffer.len());
            let read = reader.read(&mut buffer[..chunk])?;
            ensure!(read > 0, "local response body was truncated");
            body.extend_from_slice(&buffer[..read]);
            remaining_bytes -= read;
        }
    }
    Ok((outcome, status, length, body))
}

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        if let Err(error) = self.0.wait() {
            eprintln!("operation=scale phase=server_cleanup error={error}");
        }
    }
}

struct Components {
    parent: Vec<usize>,
    size: Vec<usize>,
}

impl Components {
    fn root(&mut self, mut item: usize) -> usize {
        while self.parent[item] != item {
            self.parent[item] = self.parent[self.parent[item]];
            item = self.parent[item];
        }
        item
    }

    fn join(&mut self, a: usize, b: usize) {
        let mut a = self.root(a);
        let mut b = self.root(b);
        if a == b {
            return;
        }
        if self.size[a] < self.size[b] {
            std::mem::swap(&mut a, &mut b);
        }
        self.parent[b] = a;
        self.size[a] += self.size[b];
    }
}

fn target(destination: &str, seed: u64) -> Result<usize> {
    let name = destination
        .strip_suffix(".md")
        .context("unexpected generated link extension")?;
    ensure!(
        name.len() == 32 && name.is_ascii(),
        "unexpected generated link name"
    );
    ensure!(
        u64::from_str_radix(&name[..16], 16)? == seed,
        "generated link uses another seed"
    );
    Ok(usize::from_str_radix(&name[16..], 16)?)
}

fn audit(directory: &Path, corpus: &Corpus, deadline: Instant) -> Result<()> {
    let store = tungsten::store::Store::open(directory)?;
    let mut incoming = vec![0usize; corpus.count];
    let mut outgoing = vec![0usize; corpus.count];
    let mut components = Components {
        parent: (0..corpus.count).collect(),
        size: vec![1; corpus.count],
    };
    let mut titles = HashSet::new();
    let mut duplicates = 0;
    let mut missing = 0;
    let mut links = 0;
    let mut titleless = 0;
    let mut long_pages = 0;
    let mut unicode = 0;
    for (index, degree) in outgoing.iter_mut().enumerate() {
        if index.is_multiple_of(10_000) {
            ensure!(
                Instant::now() < deadline,
                "inconclusive: dataset audit deadline exceeded"
            );
            eprintln!(
                "operation=scale phase=audit pages={index} total={}",
                corpus.count
            );
        }
        let text = store
            .read(&tungsten::domain::NoteName::parse(corpus.name(index))?)?
            .body;
        let heading = text
            .lines()
            .find_map(|line| line.strip_prefix("# "))
            .unwrap_or("");
        titleless += usize::from(heading.is_empty());
        duplicates += usize::from(!heading.is_empty() && !titles.insert(heading.to_owned()));
        long_pages += usize::from(text.len() > 4096);
        unicode += usize::from(!text.is_ascii());
        for event in Parser::new(&text) {
            if let Event::Start(Tag::Link { dest_url, .. }) = event {
                links += 1;
                let target = target(&dest_url, corpus.seed)?;
                if target >= corpus.count {
                    ensure!(corpus.dangling(index, target), "unintended dangling link");
                    missing += 1;
                    continue;
                }
                incoming[target] += 1;
                *degree += 1;
                components.join(index, target);
            }
        }
    }
    let isolated = incoming
        .iter()
        .zip(&outgoing)
        .filter(|(a, b)| **a == 0 && **b == 0)
        .count();
    let terminal = outgoing.iter().filter(|&&degree| degree == 0).count();
    let component_count = (0..corpus.count)
        .filter(|&index| components.root(index) == index)
        .count();
    let largest = components.size.iter().copied().max().unwrap_or(0);
    incoming.sort_unstable();
    println!(
        "operation=scale phase=data_verified pages={} links={links} intentional_dangling={missing} isolated={isolated} terminal={terminal} duplicate_titles={duplicates} titleless={titleless} unicode_pages={unicode} long_pages={long_pages} components={component_count} largest_component={largest} indegree_p50={} indegree_p95={} indegree_max={}",
        corpus.count,
        incoming[corpus.count / 2],
        incoming[corpus.count * 95 / 100],
        incoming[corpus.count - 1]
    );
    Ok(())
}

fn latency_gate(samples: &mut [Duration]) -> Result<()> {
    ensure!(!samples.is_empty(), "no completed measurements");
    samples.sort_unstable();
    ensure!(
        samples[(samples.len() * 95).div_ceil(100) - 1] <= Duration::from_millis(500),
        "interactive p95 exceeded 500 ms"
    );
    ensure!(
        samples[samples.len() - 1] <= Duration::from_secs(1),
        "an interactive request exceeded one second"
    );
    Ok(())
}

fn observe(path: &str, label: &str, count: usize, deadline: Instant) -> Result<serde_json::Value> {
    let mut samples = Vec::new();
    let mut bytes = 0;
    let mut value = serde_json::Value::Null;
    for _ in 0..20 {
        let started = Instant::now();
        let (outcome, status, length, body) = request(
            path,
            deadline.min(started + Duration::from_secs(REQUEST_SECONDS)),
        )?;
        ensure!(
            outcome == Outcome::Complete,
            "{label} failed: {} status={status} bytes={length}",
            outcome.name()
        );
        value = serde_json::from_slice(&body).context("parse the measured JSON response")?;
        bytes = bytes.max(length);
        samples.push(started.elapsed());
    }
    latency_gate(&mut samples).with_context(|| format!("{label} capacity gate"))?;
    println!(
        "operation=scale phase=request pages={count} endpoint={label} samples=20 response_bytes={bytes} p95_ms={} max_ms={}",
        samples[18].as_millis(),
        samples[19].as_millis()
    );
    Ok(value)
}

fn concurrent_requests(hub: &str, deadline: Instant) -> Result<()> {
    let graph = format!("/api/graph?name={hub}");
    let paths = [
        "/api/notes?q=a",
        "/api/notes?q=-Ethics",
        "/api/notes?paged=true",
        "/api/notes?q=%E6%97%A5",
        "/api/graph",
        &graph,
        "/api/notes?titles=true&q=Philosophy",
        "/api/notes?q=unfindable_capacity_sentinel_71935",
    ];
    let mut samples = Vec::new();
    for _ in 0..4 {
        let results = std::thread::scope(|scope| {
            paths
                .iter()
                .map(|path| {
                    scope.spawn(move || -> Result<Duration> {
                        let started = Instant::now();
                        let (outcome, _, _, _) = request(path, deadline)?;
                        ensure!(outcome == Outcome::Complete, "a concurrent request failed");
                        Ok(started.elapsed())
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .map_err(|_| anyhow::anyhow!("a concurrent request panicked"))?
                })
                .collect::<Result<Vec<_>>>()
        })?;
        samples.extend(results);
    }
    latency_gate(&mut samples)?;
    println!(
        "operation=scale phase=concurrent clients=8 samples={} p95_ms={} max_ms={}",
        samples.len(),
        samples[(samples.len() * 95).div_ceil(100) - 1].as_millis(),
        samples
            .last()
            .context("missing concurrent samples")?
            .as_millis()
    );
    Ok(())
}

pub(super) fn native(
    counts: &[usize],
    seed: u64,
    profile: Profile,
    reuse_data: bool,
    workload: Workload,
) -> Result<()> {
    workload.validate()?;
    let Workload {
        extremes,
        allocation_gib,
    } = workload;
    ensure!(
        std::env::consts::OS == "linux",
        "run scale profiling in the bounded Linux container"
    );
    plan(counts)?;
    let campaign = Instant::now();
    if extremes {
        ensure!(
            counts == [1_000_000],
            "extreme campaigns require --counts 1000000"
        );
    }
    let deadline = campaign + Duration::from_secs(870);
    for &count in counts {
        ensure!(
            Instant::now() < deadline,
            "inconclusive: scale campaign deadline exceeded"
        );
        let directory = Path::new("/data").join(format!(
            "{}-{seed}-{count}",
            if extremes { "extremes" } else { profile.name() }
        ));
        let corpus = Corpus::new(count, seed, profile)?;
        if extremes {
            crate::stress::prepare(&directory, &corpus, deadline, allocation_gib)?;
        } else if reuse_data && directory.is_dir() {
            let store = tungsten::store::Store::open(&directory)?;
            let imported = store.note_count()?;
            ensure!(
                imported <= count,
                "the retained fixture exceeds the selected corpus"
            );
            for index in [0, imported / 2, imported.saturating_sub(1)] {
                if imported == 0 {
                    break;
                }
                let note = corpus.note(index);
                ensure!(
                    store
                        .read(&tungsten::domain::NoteName::parse(note.name)?)?
                        .body
                        == note.body,
                    "the retained fixture belongs to another corpus"
                );
            }
            drop(store);
            if imported < count {
                crate::demo::write_notes(&directory, &corpus, deadline, allocation_gib)?;
            }
            println!("operation=scale phase=reused pages={count}");
        } else {
            fs::create_dir(&directory).context("create fresh scale dataset")?;
            let started = Instant::now();
            let written = crate::demo::write_notes(&directory, &corpus, deadline, allocation_gib)?;
            println!(
                "operation=scale phase=generated pages={count} bytes={} allocated_bytes={} elapsed_ms={}",
                written.bytes,
                written.allocated,
                started.elapsed().as_millis()
            );
        }
        if !extremes {
            audit(&directory, &corpus, deadline)?;
        }
        let startup = Instant::now();
        let mut server = Server(
            Command::new("/tungsten")
                .args([
                    "--data-dir",
                    directory.to_str().context("UTF-8 dataset path")?,
                    "--listen",
                    "127.0.0.1:18081",
                ])
                .stdin(Stdio::null())
                .spawn()?,
        );
        let startup_deadline = deadline.min(startup + Duration::from_secs(600));
        let mut ready = false;
        while Instant::now() < startup_deadline {
            if matches!(
                request(
                    "/healthz",
                    (Instant::now() + Duration::from_millis(500)).min(startup_deadline),
                ),
                Ok((Outcome::Complete, 204, 0, _))
            ) {
                ready = true;
                break;
            }
            if server.0.try_wait()?.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        println!(
            "operation=scale phase=startup pages={count} ready={ready} elapsed_ms={}",
            startup.elapsed().as_millis()
        );
        ensure!(
            ready,
            "the server did not become ready within the startup budget"
        );
        if ready {
            observe(
                &format!("/api/notes/{}", corpus.name(0)),
                "open_note",
                count,
                deadline,
            )?;
            for (path, label) in [
                ("/api/notes?q=Ethics", "search"),
                ("/api/notes?q=a", "short_search"),
                ("/api/notes?q=%E6%97%A5", "unicode_search"),
                ("/api/notes?q=-Ethics", "negative_search"),
                (
                    "/api/notes?q=unfindable_capacity_sentinel_71935",
                    "absent_search",
                ),
            ] {
                observe(path, label, count, deadline)?;
            }
            let list = observe("/api/notes?paged=true", "list", count, deadline)?;
            ensure!(
                list["total"].as_u64() == Some(count as u64),
                "list count differs from the dataset"
            );
            ensure!(
                list["notes"]
                    .as_array()
                    .is_some_and(|notes| notes.len() <= 50),
                "note page is unbounded"
            );
            let graph = observe(
                &format!("/api/graph?name={}", corpus.name(0)),
                "graph",
                count,
                deadline,
            )?;
            ensure!(
                graph["total"].as_u64() == Some(count as u64),
                "graph count differs from the dataset"
            );
            ensure!(
                graph["notes"]
                    .as_array()
                    .is_some_and(|notes| notes.len() <= 120),
                "graph nodes are unbounded"
            );
            ensure!(
                graph["edges"]
                    .as_array()
                    .is_some_and(|edges| edges.len() <= 2048),
                "graph edges are unbounded"
            );
            if extremes {
                ensure!(
                    graph["neighbors"].as_u64() == Some((count - 1) as u64),
                    "the hub must have every other note as a neighbor"
                );
            }
            observe("/api/graph", "overview", count, deadline)?;
            concurrent_requests(&corpus.name(0), deadline)?;
        }
        drop(server);
        if extremes {
            crate::stress::storage_checks(&directory, &corpus.name(0), deadline)?;
        }
    }
    println!(
        "operation=scale phase=observations_complete elapsed_ms={}",
        campaign.elapsed().as_millis()
    );
    Ok(())
}

pub(super) fn run(
    root: &Path,
    counts: &[usize],
    seed: u64,
    profile: Profile,
    dry_run: bool,
    reuse_volume: Option<&str>,
    workload: Workload,
) -> Result<()> {
    workload.validate()?;
    let Workload {
        extremes,
        allocation_gib,
    } = workload;
    plan(counts)?;
    println!(
        "operation=scale phase=plan counts={counts:?} profile={} seed={seed} cpus=2 memory_mib=2048 swap_mib=0 pids=256 seconds=900 allocation_limit_gib={} response_limit_mib=4 request_seconds=15 extremes={extremes} reuse_volume={reuse_volume:?}",
        profile.name(),
        allocation_gib
    );
    if dry_run {
        return Ok(());
    }
    ensure!(
        crate::output(root, "docker", &["context", "show"])? == "orbstack"
            || std::env::consts::OS != "macos",
        "select the OrbStack Docker context"
    );
    crate::run(
        root,
        "docker",
        &[
            "build",
            "--target",
            "scale",
            "--tag",
            "tungsten-scale:local",
            ".",
        ],
    )?;
    let image = crate::output(
        root,
        "docker",
        &[
            "image",
            "inspect",
            "tungsten-scale:local",
            "--format",
            "{{.Id}}",
        ],
    )?;
    ensure!(
        image.starts_with("sha256:"),
        "missing immutable scale image identity"
    );
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let volume = match reuse_volume {
        Some(volume) => {
            ensure!(
                volume
                    .strip_prefix("tungsten-scale-")
                    .is_some_and(|suffix| !suffix.is_empty()
                        && suffix.bytes().all(|byte| byte.is_ascii_digit())),
                "reuse only a generated scale volume"
            );
            ensure!(
                crate::output(
                    root,
                    "docker",
                    &[
                        "volume",
                        "inspect",
                        volume,
                        "--format",
                        "{{index .Labels \"org.tungsten.purpose\"}}"
                    ]
                )? == "scale-profile",
                "the scale volume has another purpose"
            );
            volume.to_owned()
        }
        None => format!("tungsten-scale-{stamp}"),
    };
    if reuse_volume.is_none() {
        crate::run(
            root,
            "docker",
            &[
                "volume",
                "create",
                "--label",
                "org.tungsten.purpose=scale-profile",
                &volume,
            ],
        )?;
    }
    let sizes = counts
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let seed = seed.to_string();
    let mount = format!("type=volume,source={volume},target=/data");
    let reuse_data = reuse_volume.is_some().to_string();
    let extremes = extremes.to_string();
    let allocation_gib = allocation_gib.to_string();
    println!("operation=scale phase=retained_data volume={volume}");
    crate::run(
        root,
        "docker",
        &[
            "run",
            "--rm",
            "--init",
            "--network=none",
            "--memory=2g",
            "--memory-swap=2g",
            "--cpus=2",
            "--pids-limit=256",
            "--read-only",
            "--tmpfs",
            "/tmp:size=268435456,mode=1777",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--mount",
            &mount,
            &image,
            "--counts",
            &sizes,
            "--seed",
            &seed,
            "--profile",
            profile.name(),
            "--reuse-data",
            &reuse_data,
            "--extremes",
            &extremes,
            "--allocation-gib",
            &allocation_gib,
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn workload_limits_reject_misreported_budgets() {
        for allocation_gib in [0, 7, 13, u64::MAX] {
            assert!(
                Workload {
                    extremes: false,
                    allocation_gib
                }
                .validate()
                .is_err()
            );
        }
        for extremes in [false, true] {
            for allocation_gib in [8, 12] {
                Workload {
                    extremes,
                    allocation_gib,
                }
                .validate()
                .unwrap();
            }
        }
    }

    #[test]
    fn profiling_plans_bound_the_entire_workload() {
        assert!(plan(&[10_000, 100_000, 1_000_000]).is_ok());
        assert!(plan(&[]).is_err());
        assert!(plan(&[1_000_001]).is_err());
        assert!(plan(&[1_000_000, 1_000_000]).is_err());
        assert!(plan(&[1_000_000, 10_000]).is_err());
    }

    #[test]
    fn capacity_gate_rejects_missing_and_slow_measurements() {
        assert!(latency_gate(&mut []).is_err());
        assert!(latency_gate(&mut [Duration::from_millis(10); 20]).is_ok());
        assert!(latency_gate(&mut [Duration::from_millis(501); 20]).is_err());
        let mut samples = [Duration::from_millis(10); 20];
        samples[0] = Duration::from_secs(2);
        assert!(latency_gate(&mut samples).is_err());
    }

    #[test]
    fn response_budgets_distinguish_results_from_completed_measurements() {
        assert_eq!(
            headers("HTTP/1.1 200 OK", &["Content-Length: 42".to_owned()]).unwrap(),
            (200, 42)
        );
        assert_eq!(headers("HTTP/1.1 204 No Content", &[]).unwrap(), (204, 0));
        assert!(headers("bad response", &[]).is_err());
        assert!(headers("HTTP/1.1 200 OK", &["Content-Length: invalid".to_owned()]).is_err());
        assert!(headers("HTTP/1.1 200 OK", &[]).is_err());
        assert_eq!(classify(200, BODY_BUDGET + 1), Outcome::ResponseBudget);
        assert_eq!(classify(503, 100), Outcome::HttpFailure);
        assert_eq!(classify(200, 100), Outcome::Complete);
    }

    #[test]
    fn local_transport_rejects_incomplete_and_oversized_responses() {
        for (response, expected) in [
            (
                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}",
                Some(Outcome::Complete),
            ),
            (
                "HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\n\r\n",
                Some(Outcome::HttpFailure),
            ),
            (
                "HTTP/1.1 200 OK\r\nContent-Length: 4194305\r\n\r\n",
                Some(Outcome::ResponseBudget),
            ),
            ("HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n{}", None),
            ("HTTP/1.1 200 OK\r\n\r\n{}", None),
            (
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
                None,
            ),
        ] {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut reader = BufReader::new(&mut stream);
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                }
                stream.write_all(response.as_bytes()).unwrap();
            });
            let result = request_at(address, "/fixture", Instant::now() + Duration::from_secs(1));
            server.join().unwrap();
            assert_eq!(result.ok().map(|value| value.0), expected, "{response}");
        }
    }

    #[test]
    fn stored_body_audits_reject_a_link_outside_the_declared_fixture() {
        let directory = tempfile::tempdir().unwrap();
        let corpus = Corpus::new(500, 42, Profile::Mixed).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        crate::demo::write_notes(directory.path(), &corpus, deadline, 8).unwrap();
        audit(directory.path(), &corpus, deadline).unwrap();
        let mut store = tungsten::store::Store::open(directory.path()).unwrap();
        let name = tungsten::domain::NoteName::parse(corpus.name(0)).unwrap();
        let note = store.read(&name).unwrap();
        store
            .update(
                &name,
                &format!("# Changed\n\n[Unexpected]({})\n", corpus.name(100_000)),
                note.revision.as_str(),
            )
            .unwrap();
        drop(store);
        assert!(audit(directory.path(), &corpus, deadline).is_err());
    }
}
