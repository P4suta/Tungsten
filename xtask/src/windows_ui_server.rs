use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::Duration,
};

pub struct Server {
    address: SocketAddr,
    requests: Arc<Mutex<Vec<String>>>,
    worker: Option<JoinHandle<Result<()>>>,
}

impl Server {
    pub fn start() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let worker = std::thread::spawn(move || {
            let mut body = "# Collection retry\n\nOriginal writing.".to_owned();
            let mut revision = format!("\"{}\"", "a".repeat(64));
            let mut searches = 0;
            for connection in listener.incoming() {
                let mut connection = connection?;
                connection.set_read_timeout(Some(Duration::from_secs(5)))?;
                connection.set_write_timeout(Some(Duration::from_secs(5)))?;
                let mut reader = BufReader::new(&mut connection);
                let mut first = String::new();
                if reader.read_line(&mut first)? == 0 {
                    break;
                }
                let mut length = 0;
                let mut bytes = first.len();
                loop {
                    let mut line = String::new();
                    ensure!(
                        reader.read_line(&mut line)? > 0,
                        "incomplete UI fixture request"
                    );
                    bytes += line.len();
                    ensure!(bytes < 16384, "oversized UI fixture headers");
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':')
                        && name.eq_ignore_ascii_case("content-length")
                    {
                        length = value.trim().parse::<usize>()?;
                    }
                }
                ensure!(length <= 4096, "oversized UI fixture body");
                let mut content = vec![0; length];
                reader.read_exact(&mut content)?;
                drop(reader);
                let mut parts = first.split_whitespace();
                let method = parts.next().context("UI fixture method")?;
                let target = parts.next().context("UI fixture target")?;
                recorded
                    .lock()
                    .map_err(|_| anyhow::anyhow!("UI request lock"))?
                    .push(target.to_owned());
                let (status, value) = if target.starts_with("/api/workspaces") {
                    (200, json!({"workspaces": [], "next": null}))
                } else if target == "/api/notes/guard.md" {
                    if method == "PUT" {
                        body = serde_json::from_slice::<Value>(&content)?["body"]
                            .as_str()
                            .context("UI fixture note body")?
                            .to_owned();
                        revision = format!("\"{}\"", "b".repeat(64));
                    }
                    (
                        200,
                        json!({"name": "guard.md", "body": body, "revision": revision}),
                    )
                } else if target.starts_with("/api/notes?") {
                    let query = target.split_once('?').context("UI fixture query")?.1;
                    if query.split('&').any(|field| field == "q=needle") {
                        searches += 1;
                    }
                    if searches == 1 && query.split('&').any(|field| field == "q=needle") {
                        (503, json!({"error": "Search temporarily unavailable."}))
                    } else {
                        (
                            200,
                            json!({"notes": [{"name": "guard.md", "title": "Collection retry", "snippet": "Original writing."}], "next": null, "total": 1}),
                        )
                    }
                } else {
                    (404, json!({"error": "Unknown UI fixture route."}))
                };
                let payload = serde_json::to_vec(&value)?;
                write!(
                    connection,
                    "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    payload.len()
                )?;
                connection.write_all(&payload)?;
            }
            Ok(())
        });
        Ok(Self {
            address,
            requests,
            worker: Some(worker),
        })
    }

    pub fn origin(&self) -> String {
        format!("http://{}/", self.address)
    }

    pub fn requests(&self) -> Result<Vec<String>> {
        Ok(self
            .requests
            .lock()
            .map_err(|_| anyhow::anyhow!("UI request lock"))?
            .clone())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = TcpStream::connect_timeout(&self.address, Duration::from_secs(2));
        if let Some(worker) = self.worker.take() {
            match worker.join() {
                Ok(Ok(())) => {}
                result => eprintln!("phase=ui-fixture-stop outcome=failed result={result:?}"),
            }
        }
    }
}
