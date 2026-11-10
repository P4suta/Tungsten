use anyhow::{Context, Result, ensure};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use tungsten::{
    domain::Revision,
    store::{Note, Store},
};

const PEAK_BUDGET_KIB: u64 = 24 * 1024;
const FILE_BYTES: usize = 64 * 1024 * 1024;
const ADDRESS: &str = "127.0.0.1:18091";

#[derive(Debug, PartialEq)]
struct Memory {
    resident_kib: u64,
    peak_kib: u64,
}

fn measurement(status: &str) -> Result<Memory> {
    fn field(status: &str, key: &str) -> Result<u64> {
        let mut fields = status.lines().filter_map(|line| line.strip_prefix(key));
        let value = fields
            .next()
            .context("missing process memory measurement")?;
        ensure!(fields.next().is_none(), "duplicate memory measurement");
        let mut words = value.split_whitespace();
        let value: u64 = words.next().context("missing memory value")?.parse()?;
        ensure!(
            words.next() == Some("kB") && words.next().is_none(),
            "invalid memory unit"
        );
        ensure!(value > 0, "empty process memory measurement");
        Ok(value)
    }
    let value = Memory {
        resident_kib: field(status, "VmRSS:")?,
        peak_kib: field(status, "VmHWM:")?,
    };
    ensure!(
        value.peak_kib >= value.resident_kib,
        "invalid memory high-water mark"
    );
    Ok(value)
}

fn budget(value: &Memory) -> Result<()> {
    ensure!(
        value.peak_kib <= PEAK_BUDGET_KIB,
        "server peak {} KiB exceeds {} KiB",
        value.peak_kib,
        PEAK_BUDGET_KIB
    );
    Ok(())
}

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        if let Ok(Some(status)) = self.0.try_wait() {
            eprintln!("operation=memory_profile phase=server_exit status={status}");
            if let Ok(events) = fs::read_to_string("/sys/fs/cgroup/memory.events") {
                eprintln!("operation=memory_profile phase=container_events {events}");
            }
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Server {
    fn sample(&mut self, phase: &str) -> Result<Memory> {
        ensure!(
            self.0.try_wait()?.is_none(),
            "memory-profile server exited during {phase}"
        );
        let value = measurement(&fs::read_to_string(format!(
            "/proc/{}/status",
            self.0.id()
        ))?)?;
        println!(
            "operation=memory_profile phase={phase} resident_kib={} peak_kib={}",
            value.resident_kib, value.peak_kib
        );
        if let Ok(current) = fs::read_to_string("/sys/fs/cgroup/memory.current") {
            println!(
                "operation=memory_profile phase=container_memory bytes={}",
                current.trim()
            );
        }
        Ok(value)
    }
}

struct Reply {
    body: Vec<u8>,
    bytes: usize,
}

fn request(
    method: &str,
    path: &str,
    length: usize,
    mut input: impl Read,
    extra: &str,
    file: bool,
) -> Result<Reply> {
    let address: SocketAddr = ADDRESS.parse()?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(1))?;
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    stream.set_write_timeout(Some(Duration::from_secs(15)))?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {length}\r\n{extra}\r\n"
    )?;
    ensure!(
        std::io::copy(&mut input, &mut stream)? == length as u64,
        "incomplete fixture request"
    );
    let mut reader = BufReader::new(stream);
    let mut status = String::new();
    reader.read_line(&mut status)?;
    let code: u16 = status
        .split_whitespace()
        .nth(1)
        .context("missing HTTP status")?
        .parse()?;
    ensure!(
        (200..300).contains(&code),
        "memory-profile request {path} failed with {code}"
    );
    let mut header_bytes = status.len();
    loop {
        let mut field = String::new();
        ensure!(
            reader.by_ref().take(8192).read_line(&mut field)? > 0,
            "truncated response headers"
        );
        header_bytes += field.len();
        ensure!(header_bytes <= 8192, "response headers exceed budget");
        if field == "\r\n" {
            break;
        }
    }
    let mut body = Vec::new();
    let mut bytes = 0;
    let mut buffer = [0; 16 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes += count;
        ensure!(
            bytes <= if file { FILE_BYTES } else { 4 * 1024 * 1024 },
            "response exceeds budget"
        );
        if file {
            ensure!(
                buffer[..count].iter().all(|byte| *byte == 0x5a),
                "attachment bytes changed"
            );
        } else {
            body.extend_from_slice(&buffer[..count]);
        }
    }
    Ok(Reply { body, bytes })
}

pub fn native(executable: &Path) -> Result<()> {
    ensure!(
        std::env::consts::OS == "linux",
        "run memory profiling in its bounded Linux container"
    );
    let directory = tempfile::tempdir_in("/data")?;
    {
        let mut store = Store::open(directory.path())?;
        store.import_notes((0..5000).map(|index| {
            let body = format!("# Memory note {index}\n\nConnected writing 日本語.\n\n[Memory note 0](Memory-0.md)\n");
            Ok(Note { name: tungsten::domain::NoteName::parse(format!("Memory-{index}.md"))?, revision: Revision::of(body.as_bytes()), body, modified: 1_735_689_600_000 + index })
        }))?;
    }
    let mut server = Server(
        Command::new(executable)
            .arg("--data-dir")
            .arg(directory.path())
            .args(["--listen", ADDRESS])
            .stdin(Stdio::null())
            .spawn()?,
    );
    let started = Instant::now();
    loop {
        if request("GET", "/healthz", 0, std::io::empty(), "", false).is_ok() {
            break;
        }
        ensure!(
            server.0.try_wait()?.is_none(),
            "memory-profile server exited during startup"
        );
        ensure!(
            started.elapsed() < Duration::from_secs(10),
            "memory-profile server startup timed out"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    server.sample("idle")?;
    for path in ["/", "/assets/app.js", "/assets/style.css"] {
        request(
            "GET",
            path,
            0,
            std::io::empty(),
            "Accept-Encoding: br\r\n",
            false,
        )?;
    }
    server.sample("assets")?;
    for _ in 0..20 {
        for path in [
            "/api/notes?paged=true",
            "/api/notes?q=Connected&paged=true",
            "/api/graph?name=Memory-0.md&limit=120",
            "/api/history?name=Memory-0.md",
        ] {
            request("GET", path, 0, std::io::empty(), "", false)?;
        }
    }
    server.sample("search")?;
    {
        let body = format!(
            "# Memory capacity\n\n{}",
            "## Heading 日本語\n\n".repeat(35_000)
        );
        let input =
            serde_json::to_vec(&serde_json::json!({"name":"Memory-capacity.md","body":body}))?;
        let reply = request(
            "POST",
            "/api/notes",
            input.len(),
            input.as_slice(),
            "Content-Type: application/json\r\n",
            false,
        )?;
        let saved: serde_json::Value = serde_json::from_slice(&reply.body)?;
        ensure!(
            saved["body"].as_str() == Some(body.as_str()),
            "large note bytes changed"
        );
        let revision = saved["revision"]
            .as_str()
            .context("missing note revision")?;
        request(
            "DELETE",
            "/api/notes/Memory-capacity.md",
            0,
            std::io::empty(),
            &format!("Content-Type: application/json\r\nIf-Match: {revision}\r\n"),
            false,
        )?;
        server.sample("large_note")?;
    }
    for attempt in 0..3 {
        let reply = request(
            "POST",
            "/api/attachments",
            FILE_BYTES,
            std::io::repeat(0x5a).take(FILE_BYTES as u64),
            "Content-Type: application/octet-stream\r\nX-Attachment-Name: memory.bin\r\n",
            false,
        )?;
        let attachment: serde_json::Value = serde_json::from_slice(&reply.body)?;
        ensure!(
            attachment["bytes"].as_u64() == Some(FILE_BYTES as u64),
            "attachment size changed"
        );
        server.sample("upload")?;
        let url = attachment["url"]
            .as_str()
            .context("missing attachment URL")?;
        let reply = request("GET", url, 0, std::io::empty(), "", true)?;
        ensure!(reply.bytes == FILE_BYTES, "truncated attachment response");
        let id = attachment["id"]
            .as_str()
            .context("missing attachment identity")?;
        let revision = attachment["revision"]
            .as_str()
            .context("missing attachment revision")?;
        request(
            "DELETE",
            &format!("/api/attachments/{id}"),
            0,
            std::io::empty(),
            &format!("Content-Type: application/json\r\nIf-Match: {revision}\r\n"),
            false,
        )?;
        println!("operation=memory_profile phase=round_trip attempt={attempt} bytes={FILE_BYTES}");
    }
    budget(&server.sample("complete")?)?;
    Ok(())
}

pub fn verify(root: &Path, dry_run: bool) -> Result<()> {
    eprintln!(
        "operation=memory_profile cpus=1 memory_mib=64 swap_mib=0 pids=32 deadline_seconds=180 notes=5000 uploads=3 file_mib=64 server_peak_mib=24"
    );
    if dry_run {
        return Ok(());
    }
    crate::run(
        root,
        "docker",
        &[
            "build",
            "--target",
            "memory-profile",
            "--tag",
            "tungsten-memory-profile:local",
            ".",
        ],
    )?;
    let image = crate::output(
        root,
        "docker",
        &[
            "image",
            "inspect",
            "--format",
            "{{.Id}}",
            "tungsten-memory-profile:local",
        ],
    )?;
    crate::run(
        root,
        "docker",
        &[
            "run",
            "--rm",
            "--init",
            "--network=none",
            "--cpus=1",
            "--memory=64m",
            "--memory-swap=64m",
            "--pids-limit=32",
            "--read-only",
            "--tmpfs",
            "/tmp:size=8388608,mode=1777",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            &image,
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_measurements_reject_missing_invalid_and_excessive_values() {
        let good = measurement("Name:\ttungsten\nVmRSS:\t4096 kB\nVmHWM:\t8192 kB\n").unwrap();
        assert_eq!(
            good,
            Memory {
                resident_kib: 4096,
                peak_kib: 8192
            }
        );
        budget(&good).unwrap();
        assert!(
            budget(&Memory {
                resident_kib: 4096,
                peak_kib: PEAK_BUDGET_KIB + 1
            })
            .is_err()
        );
        for invalid in [
            "",
            "VmRSS: 1 kB",
            "VmRSS: 0 kB\nVmHWM: 2 kB",
            "VmRSS: 2 kB\nVmHWM: 1 kB",
            "VmRSS: 1 MB\nVmHWM: 2 kB",
            "VmRSS: 1 kB\nVmRSS: 1 kB\nVmHWM: 2 kB",
        ] {
            assert!(measurement(invalid).is_err(), "{invalid}");
        }
    }
}
