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
            warn_after: Duration::from_secs(crate::timeout_policy::DEFAULT_WARN_SECS),
            nudge_after: Duration::from_secs(crate::timeout_policy::DEFAULT_NUDGE_SECS),
            escalate_after: Duration::from_secs(crate::timeout_policy::DEFAULT_ESCALATE_SECS),
        }
    }
}

impl IdleDetector {
    pub(crate) fn from_policy(policy: crate::timeout_policy::TimeoutPolicy) -> Self {
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

fn take_inbound_echo_at(
    pending: &mut Vec<InboundEcho>,
    line: &str,
    now: Instant,
) -> bool {
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
mod tests {
    use super::*;

    fn detector() -> IdleDetector {
        IdleDetector {
            warn_after: Duration::from_secs(10),
            nudge_after: Duration::from_secs(20),
            escalate_after: Duration::from_secs(30),
        }
    }

    #[test]
    fn idle_thresholds_follow_expected_transitions() {
        assert_eq!(
            detector().tick(
                Instant::now() - Duration::from_secs(9),
                MonitorTaskStatus::Running,
                false,
                true,
            ),
            IdleAction::None
        );
        assert_eq!(
            detector().tick(
                Instant::now() - Duration::from_secs(10),
                MonitorTaskStatus::Running,
                false,
                true,
            ),
            IdleAction::WarnEvent
        );
        assert_eq!(
            detector().tick(
                Instant::now() - Duration::from_secs(20),
                MonitorTaskStatus::Running,
                false,
                true,
            ),
            IdleAction::SendNudge(default_nudge_message())
        );
        assert_eq!(
            detector().tick(
                Instant::now() - Duration::from_secs(30),
                MonitorTaskStatus::Running,
                true,
                true,
            ),
            IdleAction::Escalate
        );
    }

    #[test]
    fn non_running_states_never_emit_idle_actions() {
        for status in [
            MonitorTaskStatus::AwaitingInput,
            MonitorTaskStatus::Stalled,
            MonitorTaskStatus::Inactive,
        ] {
            assert_eq!(
                detector().tick(Instant::now() - Duration::from_secs(60), status, true, true),
                IdleAction::None
            );
        }
    }

    #[test]
    fn tick_skips_nudge_for_agents_that_do_not_accept_nudges() {
        assert_eq!(
            detector().tick(
                Instant::now() - Duration::from_secs(20),
                MonitorTaskStatus::Running,
                false,
                false,
            ),
            IdleAction::None
        );
    }

    #[test]
    fn tick_escalates_without_prior_nudge_when_agent_does_not_accept_nudges() {
        assert_eq!(
            detector().tick(
                Instant::now() - Duration::from_secs(30),
                MonitorTaskStatus::Running,
                false,
                false,
            ),
            IdleAction::Escalate
        );
    }

    #[test]
    fn tick_still_sends_nudge_for_agents_that_accept_nudges() {
        assert_eq!(
            detector().tick(
                Instant::now() - Duration::from_secs(20),
                MonitorTaskStatus::Running,
                false,
                true,
            ),
            IdleAction::SendNudge(default_nudge_message())
        );
    }

    #[test]
    fn take_inbound_echo_suppresses_terminal_echo_and_immediate_duplicate() {
        let mut pending = Vec::new();
        register_inbound_echo(&mut pending, default_nudge_message());
        register_inbound_echo(&mut pending, "other".to_string());
        assert!(take_inbound_echo(&mut pending, "  Task appears idle. Status update please?  "));
        assert!(take_inbound_echo(&mut pending, &default_nudge_message()));
        assert!(!take_inbound_echo(&mut pending, &default_nudge_message()));
        assert!(take_inbound_echo(&mut pending, "other"));
        assert!(take_inbound_echo(&mut pending, "other"));
        assert!(pending.is_empty());
    }

    #[test]
    fn inbound_echo_suppression_covers_delayed_terminal_echo() {
        let registered_at = Instant::now();
        let mut pending = Vec::new();
        register_inbound_echo(&mut pending, default_nudge_message());

        assert!(take_inbound_echo_at(
            &mut pending,
            &default_nudge_message(),
            registered_at + Duration::from_secs(6),
        ));
    }

    #[test]
    fn inbound_echo_suppression_expires_after_bounded_window() {
        let registered_at = Instant::now();
        let mut pending = Vec::new();
        register_inbound_echo(&mut pending, default_nudge_message());

        assert!(!take_inbound_echo_at(
            &mut pending,
            &default_nudge_message(),
            registered_at + INBOUND_ECHO_WINDOW + Duration::from_millis(1),
        ));
        assert!(pending.is_empty());
    }

    #[test]
    fn inbound_echo_suppression_has_a_finite_pending_entry_cap() {
        let mut pending = Vec::new();
        for index in 0..=MAX_PENDING_INBOUND_ECHOES {
            register_inbound_echo(&mut pending, format!("message-{index}"));
        }
        assert_eq!(pending.len(), MAX_PENDING_INBOUND_ECHOES);
    }

    #[test]
    fn third_matching_echo_inside_window_is_real_output() {
        let mut pending = Vec::new();
        register_inbound_echo(&mut pending, default_nudge_message());
        assert!(take_inbound_echo(&mut pending, &default_nudge_message()));
        assert!(take_inbound_echo(&mut pending, &default_nudge_message()));
        assert!(!take_inbound_echo(&mut pending, &default_nudge_message()));
    }
}
