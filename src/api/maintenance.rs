use crate::store::Store;
use std::{
    io,
    sync::{
        Weak,
        mpsc::{self, RecvTimeoutError, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

pub(super) struct Maintenance {
    stop: Sender<()>,
    worker: Option<JoinHandle<()>>,
}

impl Maintenance {
    pub fn start(state: Weak<Mutex<Store>>) -> io::Result<Self> {
        let (stop, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("tungsten-maintenance".into())
            .spawn(move || {
                let mut failure = None;
                let mut interval = Duration::from_millis(20);
                loop {
                    match receiver.recv_timeout(interval) {
                        Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                        Err(RecvTimeoutError::Timeout) => {}
                    }
                    let Some(state) = state.upgrade() else { break };
                    let mut store = state.blocking_lock_owned();
                    let now = SystemTime::now();
                    let result = now
                        .duration_since(UNIX_EPOCH)
                        .map_err(io::Error::other)
                        .map_err(crate::Error::from)
                        .and_then(|now| {
                            let links = store.process_link_updates(256)?;
                            let recovery = store.process_recovery_cleanup(now.as_millis(), 8)?;
                            let files =
                                store.collect_indexed_unused_attachments(SystemTime::now(), 8)?;
                            let pages = store.reclaim_deleted_pages()?;
                            Ok(links + recovery + files + pages)
                        });
                    interval = if matches!(result, Ok(count) if count > 0) {
                        Duration::from_millis(20)
                    } else {
                        Duration::from_secs(1)
                    };
                    let error = result.err().map(|error| error.to_string());
                    if error != failure {
                        if let Some(error) = &error {
                            eprintln!("operation=maintenance outcome=failed error={error}");
                        }
                        failure = error;
                    }
                }
            })?;
        Ok(Self {
            stop,
            worker: Some(worker),
        })
    }
}

impl Drop for Maintenance {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            eprintln!("operation=maintenance outcome=worker_panicked");
        }
    }
}
