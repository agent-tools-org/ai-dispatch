// Idle policy for PTY-backed reply and unstick handling.
// Exports: IdleDetector, IdleAction, and MonitorTaskStatus for pty_watch.
// Deps: std::time and the resolved timeout policy.

use std::time::{Duration, Instant};

const DEFAULT_NUDGE_MESSAGE: &str = "Task appears idle. Status update please?";

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
    ) -> IdleAction {
        let idle_for = last_output_time.elapsed();
        if status != MonitorTaskStatus::Running || idle_for < self.warn_after {
            return IdleAction::None;
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
            ),
            IdleAction::None
        );
        assert_eq!(
            detector().tick(
                Instant::now() - Duration::from_secs(10),
                MonitorTaskStatus::Running,
                false,
            ),
            IdleAction::WarnEvent
        );
        assert_eq!(
            detector().tick(
                Instant::now() - Duration::from_secs(20),
                MonitorTaskStatus::Running,
                false,
            ),
            IdleAction::SendNudge(default_nudge_message())
        );
        assert_eq!(
            detector().tick(
                Instant::now() - Duration::from_secs(30),
                MonitorTaskStatus::Running,
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
                detector().tick(Instant::now() - Duration::from_secs(60), status, true),
                IdleAction::None
            );
        }
    }
}
