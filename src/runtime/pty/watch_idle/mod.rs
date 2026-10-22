// Idle policy for PTY-backed reply and unstick handling.
// Exports idle policy, output classification, and monitor status helpers.
// Deps: Store, task status, std::time, and the resolved timeout policy.

use anyhow::Result;
use std::time::{Duration, Instant};

use crate::store::Store;
use crate::types::TaskStatus;

const DEFAULT_NUDGE_MESSAGE: &str = "Task appears idle. Status update please?";
// Thirty seconds covers delayed PTY delivery while making hour-later repeats real output.
const INBOUND_ECHO_WINDOW: Duration = Duration::from_secs(30);
const INBOUND_ECHO_MATCHES: u8 = 2;
const MAX_PENDING_INBOUND_ECHOES: usize = 64;

// Invariant: each message suppresses at most two matching lines for 30 seconds, with 64 pending messages maximum.
#[derive(Debug, Clone)]
pub(crate) struct InboundEcho {
    message: String,
    expires_at: Instant,
    remaining_matches: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MonitorTaskStatus {
    Running,
    AwaitingInput,
    Stalled,
    Inactive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IdleAction {
    None,
    WarnEvent,
    SendNudge(String),
    Escalate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IdleDetector {
    pub(crate) warn_after: Duration,
    pub(crate) nudge_after: Duration,
    pub(crate) escalate_after: Duration,
}

impl Default for IdleDetector {
    fn default() -> Self {
        Self {
            warn_after: Duration::from_secs(crate::config::timeout_policy::DEFAULT_WARN_SECS),
            nudge_after: Duration::from_secs(crate::config::timeout_policy::DEFAULT_NUDGE_SECS),
            escalate_after: Duration::from_secs(
                crate::config::timeout_policy::DEFAULT_ESCALATE_SECS,
            ),
        }
    }
}

impl IdleDetector {
    pub(crate) fn from_policy(policy: crate::config::timeout_policy::TimeoutPolicy) -> Self {
        Self {
            warn_after: policy.nudge_ladder.warn,
            nudge_after: policy.nudge_ladder.nudge,
            escalate_after: policy.nudge_ladder.escalate,
        }
    }

    pub(crate) fn tick(
        &self,
        last_output_time: Instant,
        status: MonitorTaskStatus,
        idle_nudged: bool,
        accepts_nudge: bool,
    ) -> IdleAction {
        let idle_for = last_output_time.elapsed();
        if status != MonitorTaskStatus::Running || idle_for < self.warn_after {
            return IdleAction::None;
        }
        if !accepts_nudge {
            return if idle_for >= self.escalate_after {
                IdleAction::Escalate
            } else if idle_for >= self.nudge_after {
                IdleAction::None
            } else {
                IdleAction::WarnEvent
            };
        }
        if idle_for >= self.escalate_after {
            return if idle_nudged {
                IdleAction::Escalate
            } else {
                IdleAction::SendNudge(default_nudge_message())
            };
        }
        if idle_for >= self.nudge_after {
            return if idle_nudged {
                IdleAction::None
            } else {
                IdleAction::SendNudge(default_nudge_message())
            };
        }
        IdleAction::WarnEvent
    }
}

pub(crate) fn default_nudge_message() -> String {
    DEFAULT_NUDGE_MESSAGE.to_string()
}

pub(crate) fn register_inbound_echo(pending: &mut Vec<InboundEcho>, message: String) {
    let now = Instant::now();
    pending.retain(|echo| echo.expires_at > now && echo.remaining_matches > 0);
    if pending.len() >= MAX_PENDING_INBOUND_ECHOES {
        pending.remove(0);
    }
    pending.push(InboundEcho {
        message,
        expires_at: now + INBOUND_ECHO_WINDOW,
        remaining_matches: INBOUND_ECHO_MATCHES,
    });
}

/// True when a flushed stream line is meaningful agent output.
///
/// Raw/unparsed output still proves the agent is alive and working, so it must
/// refresh the idle clock. Lines that carry no text after terminal-escape
/// stripping are terminal-control noise (spinners, cursor hides) and must not
/// keep a wedged process alive.
pub(crate) fn is_agent_output(line: &str) -> bool {
    !line.trim().is_empty()
}

pub(crate) fn load_monitor_status(store: &Store, task_id: &str) -> Result<MonitorTaskStatus> {
    let status = store.get_task(task_id)?.map(|task| task.status);
    Ok(match status {
        Some(TaskStatus::Running) => MonitorTaskStatus::Running,
        Some(TaskStatus::AwaitingInput) => MonitorTaskStatus::AwaitingInput,
        Some(TaskStatus::Stalled) => MonitorTaskStatus::Stalled,
        _ => MonitorTaskStatus::Inactive,
    })
}

/// True when a stream line is only an echo of text aid itself wrote to the PTY.
/// Those echoes must not reset the idle / hung clocks (see idle-watchdog self-nudge).
/// The bounded budget prevents a one-hour repeat from being mistaken for an echo.
pub(crate) fn take_inbound_echo(pending: &mut Vec<InboundEcho>, line: &str) -> bool {
    take_inbound_echo_at(pending, line, Instant::now())
}

fn take_inbound_echo_at(pending: &mut Vec<InboundEcho>, line: &str, now: Instant) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    pending.retain(|echo| echo.expires_at > now && echo.remaining_matches > 0);
    if let Some(idx) = pending
        .iter()
        .position(|echo| echo.message.trim() == trimmed)
    {
        if pending[idx].remaining_matches == 1 {
            pending.remove(idx);
        } else {
            pending[idx].remaining_matches -= 1;
        }
        true
    } else {
        false
    }
}

#[cfg(test)]
#[path = "idle_tests.rs"]
mod tests;
