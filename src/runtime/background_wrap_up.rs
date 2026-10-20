// Worker-owned wrap-up scheduling independent of runner transport and async polling.
// Cancels and joins the timer when execution finishes or its future is dropped.

use crate::config::timeout_policy::TimeoutPolicy;
use crate::store::Store;
use anyhow::Result;
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub(super) struct Timer {
    stop: mpsc::Sender<()>,
    worker: Option<JoinHandle<()>>,
}

impl Timer {
    pub(super) fn start(
        store: Arc<Store>,
        task_id: &str,
        start: Instant,
        policy: TimeoutPolicy,
    ) -> Result<Option<Self>> {
        let limit = policy.max_duration.min(policy.hard_cap);
        if limit.as_secs() < 600 {
            return Ok(None);
        }
        let warning = start + limit - Duration::from_secs(limit.as_secs() / 10);
        let task_id = task_id.to_string();
        let (stop, cancelled) = mpsc::channel();
        #[cfg(test)]
        let home = crate::support::paths::aid_dir();
        let worker = std::thread::Builder::new()
            .name("aid-wrap-up".into())
            .spawn(move || {
                #[cfg(test)]
                let _home = crate::support::paths::AidHomeGuard::set(&home);
                if matches!(
                    cancelled.recv_timeout(warning.saturating_duration_since(Instant::now())),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    if let Err(error) = crate::cmd::steer::wrap_up_before_timeout(
                        &store,
                        &task_id,
                        start.elapsed().as_secs(),
                        limit.as_secs(),
                    ) {
                        aid_warn!("[aid] Could not queue wrap-up for {task_id}: {error}");
                    }
                }
            })?;
        Ok(Some(Self {
            stop,
            worker: Some(worker),
        }))
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                aid_warn!("[aid] Wrap-up timer thread panicked");
            }
        }
    }
}
