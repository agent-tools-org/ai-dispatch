// Shared foreground PTY interruption state.
// Exports a small PID/interrupt control used by the signal guard and PTY runner.
// Deps: tokio time plus std synchronization primitives.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

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

    pub(crate) fn is_interrupted(&self) -> bool {
        self.interrupted.load(Ordering::SeqCst)
    }
}
