// Verification time ownership through command preparation, lock acquisition and wait.
// Exports: internal mutually exclusive duration/deadline budget and no-launch result.
// Deps: chrono, ProcessGuard, VerifyResult and std time/process types.

use anyhow::Result;
use chrono::{DateTime, Local};
use std::process::ExitStatus;
use std::time::Duration;

use super::VerifyResult;
use crate::process_guard::ProcessGuard;

#[derive(Clone, Copy)]
pub(crate) enum VerifyBudget {
    Duration(Duration),
    Deadline(DateTime<Local>),
}

impl VerifyBudget {
    pub(super) fn expired(self) -> bool {
        matches!(self, Self::Deadline(deadline) if Local::now() >= deadline)
    }

    pub(super) fn wait(self, guard: &mut ProcessGuard) -> Result<(Option<ExitStatus>, Duration)> {
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
            exit_code: None,
        }
    }
}
