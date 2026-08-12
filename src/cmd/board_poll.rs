// Board anti-polling cooldown and force-escalation markers.
// Exports: anti_poll_status helpers used by `aid board`.
// Deps: std path/fs.

use std::path::Path;

use crate::types::Task;

pub(super) const BOARD_MIN_COOLDOWN_SECS: i64 = 10;
pub(super) const BOARD_FORCE_COOLDOWN_SECS: i64 = 30;
const BOARD_REPEAT_LIMIT: u32 = 1;
const FORCE_ESCALATION_LIMIT: u32 = 3;
const FORCE_ESCALATION_WINDOW_SECS: i64 = 120;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum AntiPollStatus { Allowed(u32), Cooldown(i64), Repeat(u32), ForceCooldown(i64), ForceBlocked }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ForceMarkerState {
    pub count: u32,
    pub window_start: i64,
}

pub(super) fn task_fingerprint(tasks: &[Task]) -> String {
    let mut parts: Vec<String> = tasks.iter().map(|t| format!("{}:{}", t.id, t.status.label())).collect();
    parts.sort();
    parts.join(",")
}

pub(super) fn watch_instead_of_polling_hint(tasks: &[Task]) -> String {
    use crate::types::TaskStatus;
    let running_ids: Vec<&str> = tasks.iter().filter(|task| task.status == TaskStatus::Running).map(|task| task.id.as_str()).collect();
    if running_ids.is_empty() {
        return "Use `aid watch --wait <id>` instead of polling.".to_string();
    }
    if running_ids.len() == 1 {
        return format!("Use `aid watch --wait {}` instead of polling.", running_ids[0]);
    }
    let commands = running_ids.iter().map(|task_id| format!("`aid watch --wait {task_id}`")).collect::<Vec<_>>().join(", ");
    format!("Use one of {commands} instead of polling.")
}

pub(super) fn anti_poll_status(marker_path: &Path, fingerprint: &str, now: i64, force: bool) -> (AntiPollStatus, ForceMarkerState) {
    let marker = read_board_marker(marker_path);
    let elapsed = now - marker.timestamp;
    if force {
        let force_state = next_force_state(&marker, now);
        if elapsed >= 0 && elapsed < BOARD_FORCE_COOLDOWN_SECS { return (AntiPollStatus::ForceCooldown(elapsed), force_state) }
        if is_force_window_active(marker.force_window_start, now) && marker.force_count >= FORCE_ESCALATION_LIMIT {
            return (AntiPollStatus::ForceBlocked, force_state);
        }
        return (AntiPollStatus::Allowed(0), force_state);
    }
    if elapsed >= 0 && elapsed < BOARD_MIN_COOLDOWN_SECS { return (AntiPollStatus::Cooldown(elapsed), ForceMarkerState::default()) }
    if marker.is_uninitialized() {
        return (AntiPollStatus::Allowed(0), ForceMarkerState::default());
    }
    if marker.fingerprint == fingerprint {
        let repeat_count = marker.repeat_count + 1;
        if repeat_count >= BOARD_REPEAT_LIMIT { return (AntiPollStatus::Repeat(repeat_count), ForceMarkerState::default()) }
        return (AntiPollStatus::Allowed(repeat_count), ForceMarkerState::default());
    }
    (AntiPollStatus::Allowed(0), ForceMarkerState::default())
}

#[derive(Debug, Default)]
struct BoardMarker {
    timestamp: i64,
    fingerprint: String,
    repeat_count: u32,
    force_count: u32,
    force_window_start: i64,
}

impl BoardMarker {
    fn is_uninitialized(&self) -> bool {
        self.timestamp == 0 && self.fingerprint.is_empty() && self.repeat_count == 0 && self.force_count == 0 && self.force_window_start == 0
    }
}

fn read_board_marker(marker_path: &Path) -> BoardMarker {
    let Ok(prev) = std::fs::read_to_string(marker_path) else { return BoardMarker::default() };
    let parts: Vec<&str> = prev.lines().collect();
    BoardMarker {
        timestamp: parts.first().and_then(|s| s.parse().ok()).unwrap_or(0),
        fingerprint: parts.get(1).copied().unwrap_or("").to_string(),
        repeat_count: parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0),
        force_count: parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0),
        force_window_start: parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0),
    }
}

fn next_force_state(marker: &BoardMarker, now: i64) -> ForceMarkerState {
    if is_force_window_active(marker.force_window_start, now) {
        if marker.force_count >= FORCE_ESCALATION_LIMIT {
            return ForceMarkerState { count: marker.force_count, window_start: marker.force_window_start };
        }
        return ForceMarkerState { count: marker.force_count + 1, window_start: marker.force_window_start };
    }
    ForceMarkerState { count: 1, window_start: now }
}

fn is_force_window_active(force_window_start: i64, now: i64) -> bool {
    force_window_start > 0 && now - force_window_start >= 0 && now - force_window_start < FORCE_ESCALATION_WINDOW_SECS
}

pub(super) fn write_board_marker(marker_path: &Path, fingerprint: &str, now: i64, repeat_count: u32, force_count: u32, force_window_start: i64) {
    let _ = std::fs::write(marker_path, format!("{now}\n{fingerprint}\n{repeat_count}\n{force_count}\n{force_window_start}"));
}
