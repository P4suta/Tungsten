use super::runner::check_budget;
use anyhow::{Context, Result};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub(super) struct AllocationWatch {
    thread: Option<(mpsc::Sender<()>, JoinHandle<Result<()>>)>,
    peak: Arc<AtomicU64>,
}

impl AllocationWatch {
    pub fn start(
        path: &Path,
        limit: u64,
        failure: impl FnOnce(&anyhow::Error) + Send + 'static,
    ) -> Result<Self> {
        let path = path.to_owned();
        let (stop, receiver) = mpsc::channel();
        let peak = Arc::new(AtomicU64::new(0));
        let observed = Arc::clone(&peak);
        let thread = thread::Builder::new()
            .name("storage-allocation".into())
            .spawn(move || {
                loop {
                    match check_budget(&path, limit) {
                        Ok(bytes) => {
                            observed.fetch_max(bytes, Ordering::Relaxed);
                        }
                        Err(error) => {
                            failure(&error);
                            return Err(error);
                        }
                    }
                    match receiver.recv_timeout(Duration::from_millis(250)) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                }
            })
            .context("start storage allocation monitor")?;
        Ok(Self {
            thread: Some((stop, thread)),
            peak,
        })
    }

    pub fn finish(mut self) -> Result<u64> {
        if let Some((stop, thread)) = self.thread.take() {
            drop(stop);
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("allocation monitor panicked"))??;
        }
        Ok(self.peak.load(Ordering::Relaxed))
    }
}

impl Drop for AllocationWatch {
    fn drop(&mut self) {
        if let Some((stop, thread)) = self.thread.take() {
            drop(stop);
            match thread.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    eprintln!("operation=allocation_watch outcome=failed error={error:#}")
                }
                Err(_) => eprintln!("operation=allocation_watch outcome=panicked"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn allocation_monitor_observes_real_files_and_preserves_its_failure() {
        let directory = tempfile::tempdir().unwrap();
        let mut file = std::fs::File::create(directory.path().join("payload")).unwrap();
        file.write_all(&[42; 65536]).unwrap();
        file.sync_all().unwrap();
        let (sender, receiver) = mpsc::channel();
        let watch = AllocationWatch::start(directory.path(), 0, move |error| {
            sender.send(error.to_string()).unwrap()
        })
        .unwrap();
        assert!(
            receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .contains("exceeds")
        );
        assert!(watch.finish().is_err());
        let watch = AllocationWatch::start(directory.path(), 1024 * 1024, |_| {}).unwrap();
        assert!(watch.finish().unwrap() > 0);
    }
}
