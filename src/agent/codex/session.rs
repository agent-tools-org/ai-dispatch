// Resolve durable Codex rollouts and report missing-session fallback.
// Exports rollout and resume helpers; depends on chrono, filesystem, and task events.

use super::*;

pub(crate) const RESUME_FALLBACK_DETAIL: &str =
    "Codex session resume skipped: rollout missing; starting fresh session";
const ROLLOUT_TIMESTAMP_FORMAT: &str = "%Y-%m-%dT%H-%M-%S";

pub(crate) fn durable_session_rollout_exists(session_id: &str) -> bool {
    let Ok(real_home) = crate::agent::home_isolation::resolve_real_home() else {
        return false;
    };
    session_rollout_exists(&real_home.join(".codex").join("sessions"), session_id)
}

pub(crate) fn resume_fallback_needed(session_id: &str) -> bool {
    !durable_session_rollout_exists(session_id)
}

pub(crate) fn session_rollout_exists(sessions_dir: &Path, session_id: &str) -> bool {
    let Ok(entries) = fs::read_dir(sessions_dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            return session_rollout_exists(&path, session_id);
        }
        rollout_filename_matches(&path, session_id)
    })
}

pub(crate) fn rollout_filename_matches(path: &Path, session_id: &str) -> bool {
    if session_id.is_empty() {
        return false;
    }
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(stem) = name
        .strip_prefix("rollout-")
        .and_then(|name| name.strip_suffix(".jsonl"))
    else {
        return false;
    };
    let Some(timestamp) = stem.strip_suffix(&format!("-{session_id}")) else {
        return false;
    };
    // Resume safety intentionally depends on Codex's current timestamp-shaped rollout prefix.
    NaiveDateTime::parse_from_str(timestamp, ROLLOUT_TIMESTAMP_FORMAT).is_ok()
}

pub(crate) fn rollout_filename_matches_for_attribution(
    path: &Path,
    session_id: &str,
) -> bool {
    if session_id.is_empty() {
        return false;
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.starts_with("rollout-") && name.ends_with(&format!("-{session_id}.jsonl"))
        })
}

pub(crate) fn resume_fallback_event(task_id: &TaskId) -> TaskEvent {
    TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Milestone,
        detail: RESUME_FALLBACK_DETAIL.to_string(),
        metadata: None,
    }
}
