use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpStream},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

struct OwnedServer(Child);
impl Drop for OwnedServer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn termination_releases_the_directory_even_with_an_unfinished_upload() {
    let directory = tempfile::tempdir().unwrap();
    let mut server = OwnedServer(
        Command::new(env!("CARGO_BIN_EXE_tungsten"))
            .args(["--plaintext", "--data-dir"])
            .arg(directory.path())
            .args(["--listen", "127.0.0.1:0"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stderr = server.0.stderr.take().unwrap();
    let (ready, receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            if let Some(address) = line.strip_prefix("operation=listen outcome=ready address=") {
                let _ = ready.send(address.parse::<SocketAddr>().unwrap());
            }
        }
    });
    let address = receiver.recv_timeout(Duration::from_secs(10)).unwrap();
    let mut stalled = TcpStream::connect(address).unwrap();
    stalled.set_nodelay(true).unwrap();
    stalled
        .set_write_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    stalled
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stalled.write_all(format!("POST /api/attachments HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/octet-stream\r\nX-Attachment-Name: stalled.bin\r\nContent-Length: 1048576\r\nExpect: 100-continue\r\n\r\n").as_bytes()).unwrap();
    {
        let mut acknowledgement = BufReader::new(&mut stalled);
        let mut line = String::new();
        acknowledgement.read_line(&mut line).unwrap();
        assert_eq!(line, "HTTP/1.1 100 Continue\r\n");
        line.clear();
        acknowledgement.read_line(&mut line).unwrap();
        assert_eq!(line, "\r\n");
    }
    stalled.write_all(b"x").unwrap();
    let mut probe = TcpStream::connect(address).unwrap();
    probe
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    probe
        .set_write_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    write!(probe,"POST /api/attachments HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/octet-stream\r\nX-Attachment-Name: probe.bin\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
    let mut response = String::new();
    probe.take(16384).read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 503"), "{response}");
    assert!(
        Command::new("kill")
            .args(["-TERM", &server.0.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        if let Some(status) = server.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "shutdown kept the data directory locked"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(stalled);
    reader.join().unwrap();
    assert!(tungsten::store::Store::open(directory.path()).is_ok());
}
