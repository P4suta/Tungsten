use crate::{output, run};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::os::unix::fs::MetadataExt;
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Deserialize)]
struct Category {
    score: f64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Audit {
    numeric_value: Option<f64>,
}
#[derive(Deserialize)]
struct Report {
    categories: BTreeMap<String, Category>,
    audits: BTreeMap<String, Audit>,
}

fn check(bytes: &[u8]) -> Result<()> {
    let report: Report = serde_json::from_slice(bytes).context("read Lighthouse report")?;
    for (name, minimum) in [
        ("performance", 0.95),
        ("accessibility", 1.0),
        ("best-practices", 1.0),
        ("seo", 0.9),
    ] {
        let score = report
            .categories
            .get(name)
            .with_context(|| format!("missing Lighthouse category: {name}"))?
            .score;
        ensure!(
            score.is_finite() && score >= minimum,
            "Lighthouse {name}: {score} is below {minimum}"
        );
        eprintln!("operation=lighthouse category={name} score={score}");
    }
    for (name, maximum) in [
        ("first-contentful-paint", 1800.0),
        ("largest-contentful-paint", 2500.0),
        ("total-blocking-time", 150.0),
        ("cumulative-layout-shift", 0.05),
    ] {
        let value = report
            .audits
            .get(name)
            .and_then(|audit| audit.numeric_value)
            .with_context(|| format!("missing Lighthouse metric: {name}"))?;
        ensure!(
            value.is_finite() && value >= 0.0 && value <= maximum,
            "Lighthouse {name}: {value} exceeds {maximum}"
        );
        eprintln!("operation=lighthouse metric={name} value={value}");
    }
    Ok(())
}

pub fn verify(root: &Path, dry_run: bool) -> Result<()> {
    eprintln!(
        "operation=browser-performance cpus=2 memory=2g swap=0 processes=256 deadline_seconds=590 profiles=mobile,desktop pages=blank,markdown,trash"
    );
    if dry_run {
        return Ok(());
    }
    run(
        root,
        "docker",
        &[
            "build",
            "--target",
            "browser-performance",
            "--tag",
            "tungsten-browser-performance:local",
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
            "tungsten-browser-performance:local",
        ],
    )?;
    fs::create_dir_all(root.join("target"))?;
    let reports = tempfile::Builder::new()
        .prefix("browser-performance-")
        .tempdir_in(root.join("target"))?
        .keep();
    let mount = format!("type=bind,source={},target=/reports", reports.display());
    let owner = fs::metadata(&reports)?;
    let user = format!("{}:{}", owner.uid(), owner.gid());
    eprintln!(
        "operation=browser-performance reports={}",
        reports.display()
    );
    run(
        root,
        "docker",
        &[
            "run",
            "--rm",
            "--init",
            "--network=none",
            "--cpus=2",
            "--memory=2g",
            "--memory-swap=2g",
            "--pids-limit=256",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--user",
            &user,
            "--mount",
            &mount,
            &image,
        ],
    )?;
    for profile in ["mobile", "desktop"] {
        for page in ["blank", "markdown", "trash"] {
            check(&fs::read(
                reports.join(format!("{profile}-{page}.report.json")),
            )?)?;
        }
    }
    Ok(())
}

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn ready(address: SocketAddr) -> bool {
    let Ok(mut socket) = TcpStream::connect_timeout(&address, Duration::from_millis(100)) else {
        return false;
    };
    let timeout = Some(Duration::from_millis(200));
    if socket.set_read_timeout(timeout).is_err() || socket.set_write_timeout(timeout).is_err() {
        return false;
    }
    if socket
        .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut response = [0; 64];
    socket
        .read(&mut response)
        .is_ok_and(|length| response[..length].starts_with(b"HTTP/1.1 204"))
}
pub fn native() -> Result<()> {
    ensure!(
        std::env::consts::OS == "linux",
        "run browser performance in its bounded container"
    );
    let data = tempfile::tempdir()?;
    let mut store = tungsten::store::Store::open(data.path())?;
    store.create(Some("Empty.md".into()), "")?;
    store.create(Some("Writing.md".into()), "# Writing\n\nA private notebook with **bold text**, *emphasis*, and a [linked thought](Thought.md).\n\n## List\n\n- One\n- Two\n- [ ] A task\n\n## Code\n\n```js\nconst answer = 42;\nconsole.log(answer);\n```\n\n| Name | Value |\n| --- | --- |\n| Example | 42 |\n")?;
    store.create(
        Some("Thought.md".into()),
        "# Linked thought\n\nA connected page.\n",
    )?;
    drop(store);
    let mut server = Server(
        Command::new("/tungsten")
            .args(["--data-dir"])
            .arg(data.path())
            .args(["--listen", "127.0.0.1:18090"])
            .stdin(Stdio::null())
            .spawn()?,
    );
    let address = "127.0.0.1:18090".parse()?;
    let started = Instant::now();
    while !ready(address) {
        ensure!(server.0.try_wait()?.is_none(), "performance server exited");
        ensure!(
            started.elapsed() < Duration::from_secs(10),
            "performance server did not become ready"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    for profile in ["mobile", "desktop"] {
        for (page, path) in [
            ("blank", "/?note=Empty.md"),
            ("markdown", "/?note=Writing.md"),
            ("trash", "/trash"),
        ] {
            let destination = format!("/reports/{profile}-{page}");
            let url = format!("http://127.0.0.1:18090{path}");
            let mut command = Command::new("node");
            command
                .args([
                    "/ui/assurance/node_modules/lighthouse/cli/index.js",
                    &url,
                    "--quiet",
                    "--output=json",
                    "--output=html",
                    "--chrome-flags=--headless=new --no-sandbox --disable-dev-shm-usage",
                ])
                .arg(format!("--output-path={destination}"));
            if profile == "desktop" {
                command.arg("--preset=desktop");
            } else {
                command.arg("--throttling.cpuSlowdownMultiplier=6");
            }
            ensure!(
                command.status()?.success(),
                "Lighthouse failed for {profile}/{page}"
            );
            check(&fs::read(format!("{destination}.report.json"))?)?;
            eprintln!("operation=browser-performance outcome=passed profile={profile} page={page}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn passing() -> serde_json::Value {
        json!({"categories": {"performance":{"score":1.0},"accessibility":{"score":1.0},"best-practices":{"score":1.0},"seo":{"score":1.0}},
          "audits":{"first-contentful-paint":{"numericValue":500.0},"largest-contentful-paint":{"numericValue":600.0},"total-blocking-time":{"numericValue":0.0},"cumulative-layout-shift":{"numericValue":0.0}}})
    }
    #[test]
    fn performance_budgets_reject_missing_failed_or_incomplete_measurements() {
        let good = passing();
        assert!(check(&serde_json::to_vec(&good).unwrap()).is_ok());
        for category in ["performance", "accessibility", "best-practices", "seo"] {
            let mut bad = passing();
            bad["categories"][category]["score"] = json!(0.5);
            assert!(check(&serde_json::to_vec(&bad).unwrap()).is_err());
        }
        for metric in [
            "first-contentful-paint",
            "largest-contentful-paint",
            "total-blocking-time",
            "cumulative-layout-shift",
        ] {
            let mut bad = passing();
            bad["audits"][metric]["numericValue"] = json!(99999);
            assert!(check(&serde_json::to_vec(&bad).unwrap()).is_err());
            bad["audits"].as_object_mut().unwrap().remove(metric);
            assert!(check(&serde_json::to_vec(&bad).unwrap()).is_err());
        }
        assert!(check(b"{}").is_err());
        assert!(check(b"null").is_err());
    }
}
