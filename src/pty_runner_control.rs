// Shared foreground PTY interruption state.
// Exports a small PID/interrupt control used by the signal guard and PTY runner.
// Deps: tokio time plus std synchronization primitives.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Default)]
pub(crate) struct PtyRunControl {
    agent_pid: Arc<Mutex<Option<u32>>>,
    interrupted: Arc<AtomicBool>,
}

impl PtyRunControl {
    pub(crate) fn set_agent_pid(&self, pid: u32) {
        if let Ok(mut agent_pid) = self.agent_pid.lock() {
            *agent_pid = Some(pid);
        }
    }

    pub(crate) fn mark_interrupted(&self) {
        self.interrupted.store(true, Ordering::SeqCst);
    }

    pub(crate) fn is_interrupted(&self) -> bool {
        self.interrupted.load(Ordering::SeqCst)
    }

    pub(crate) fn agent_pid(&self) -> Option<u32> {
        self.agent_pid.lock().ok().and_then(|agent_pid| *agent_pid)
    }

    pub(crate) async fn wait_agent_pid(&self, timeout: Duration) -> Option<u32> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if let Some(pid) = self.agent_pid() {
                return Some(pid);
            }
            if tokio::time::Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
