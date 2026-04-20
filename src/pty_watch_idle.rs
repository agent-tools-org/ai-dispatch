// Idle policy for PTY-backed reply and unstick handling.
// Exports: IdleDetector, IdleAction, and MonitorTaskStatus for pty_watch.
// Deps: std::time and toml parsing for project overrides.

use std::time::{Duration, Instant};

const DEFAULT_WARN_AFTER: Duration = Duration::from_secs(180);
const DEFAULT_NUDGE_AFTER: Duration = Duration::from_secs(300);
const DEFAULT_ESCALATE_AFTER: Duration = Duration::from_secs(600);
const PROJECT_PATH: &str = ".aid/project.toml";
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
            warn_after: DEFAULT_WARN_AFTER,
            nudge_after: DEFAULT_NUDGE_AFTER,
            escalate_after: DEFAULT_ESCALATE_AFTER,
        }
    }
}

impl IdleDetector {
    pub(crate) fn load() -> Self {
        let mut detector = Self::default();
        let Ok(content) = std::fs::read_to_string(PROJECT_PATH) else {
            return detector;
        };
        let Ok(value) = content.parse::<toml::Value>() else {
            return detector;
        };
        if let Some(value) = duration_key(
            &value,
            &[&["project", "unstick", "warn_after_secs"], &["project", "idle_warn_secs"]],
        ) {
            detector.warn_after = value;
        }
        if let Some(value) = duration_key(
            &value,
            &[&["project", "unstick", "nudge_after_secs"], &["project", "idle_nudge_secs"]],
        ) {
            detector.nudge_after = value;
        }
        if let Some(value) = duration_key(
            &value,
            &[
                &["project", "unstick", "escalate_after_secs"],
                &["project", "idle_escalate_secs"],
            ],
        ) {
            detector.escalate_after = value;
        }
        detector
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

fn duration_key(value: &toml::Value, paths: &[&[&str]]) -> Option<Duration> {
    paths
        .iter()
        .find_map(|path| lookup_value(value, path).and_then(toml::Value::as_integer))
        .and_then(|secs| u64::try_from(secs).ok())
        .map(Duration::from_secs)
}

fn lookup_value<'a>(value: &'a toml::Value, path: &[&str]) -> Option<&'a toml::Value> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
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
