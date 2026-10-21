// Verification time ownership through command preparation, lock acquisition and wait.
// Exports: internal mutually exclusive duration/deadline budget and no-launch result.
// Deps: chrono, ProcessGuard, VerifyResult and std time/process types.

use anyhow::Result;
use chrono::{DateTime, Local};
use std::process::ExitStatus;
use std::time::Duration;

use super::VerifyResult;
use crate::runtime::process::guard::ProcessGuard;
use crate::runtime::remote_build::LockQueue;

#[derive(Clone, Copy)]
pub(crate) enum VerifyBudget {
    Duration(Duration),
    Deadline(DateTime<Local>),
}

impl VerifyBudget {
    pub(super) fn expired(self) -> bool {
        matches!(self, Self::Deadline(deadline) if Local::now() >= deadline)
    }

    pub(super) fn wait(
        self,
        guard: &mut ProcessGuard,
        queue: &LockQueue,
    ) -> Result<(Option<ExitStatus>, Duration)> {
        let timeout = match self {
            Self::Duration(duration) => duration,
            Self::Deadline(deadline) => deadline
                .signed_duration_since(Local::now())
                .to_std()
                .unwrap_or_default(),
        };
        let status = if matches!(self, Self::Deadline(_)) && timeout.is_zero() {
            guard.force_kill();
            guard.wait()?;
            None
        } else if matches!(self, Self::Duration(_)) {
            wait_duration(guard, timeout, queue)?
        } else {
            guard.wait_with_timeout(timeout)?
        };
        // A delayed timer must not turn an observed post-deadline exit into success.
        Ok((status.filter(|_| !self.expired()), timeout))
    }

    pub(crate) fn no_launch(command: String) -> VerifyResult {
        VerifyResult {
            success: false,
            timed_out: true,
            output: "Task deadline exhausted before remote verification; no command launched"
                .into(),
            command,
            infrastructure_failure: false,
            rbox_result: None,
            exit_code: None,
        }
    }
}

fn wait_duration(
    guard: &mut ProcessGuard,
    timeout: Duration,
    queue: &LockQueue,
) -> Result<Option<ExitStatus>> {
    let start = std::time::Instant::now();
    loop {
        let expired = start.elapsed().saturating_sub(queue.elapsed()) >= timeout;
        if guard.child_mut().try_wait()?.is_some() {
            let status = guard.wait()?;
            return Ok(if expired { None } else { Some(status) });
        }
        if expired {
            guard.force_kill();
            guard.wait()?;
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
