// Orphaned background-task staleness cleanup.
// Exports cleanup_orphaned_idle_tasks for background zombie reconciliation.
// Deps: background specs, idle timeout defaults, store events, and task types.

use anyhow::Result;
use chrono::{DateTime, Local};

use super::background_kill::terminate_task_processes;
use super::background_spec::{load_spec_for_reaper, BackgroundRunSpec};
use crate::idle_timeout::DEFAULT_IDLE_TIMEOUT_SECS;
use crate::process_monitor;
use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent, TaskId, TaskStatus};

const LIVE_WORKER_IDLE_MARGIN: u64 = 2;

pub(super) fn cleanup_orphaned_idle_tasks<F>(
    store: &Store,
    running_tasks: &[Task],
    already_cleaned: &[String],
    is_process_alive: &F,
) -> Result<Vec<String>>
where
    F: Fn(u32) -> bool,
{
    let now = Local::now();
    let mut cleaned = Vec::new();
    for task in running_tasks {
        let task_id = task.id.as_str();
        if already_cleaned.iter().any(|id| id == task_id) {
            continue;
        }
        let Ok(spec) = load_spec_for_reaper(task_id) else { continue; };
        if spec.is_none()
            && (now - task.created_at).num_hours() > crate::timeout_policy::DEFAULT_HARD_CAP_HOURS
        {
            continue;
        }
        if spec.as_ref().and_then(|spec| spec.worker_pid).is_some_and(is_process_alive) {
            continue;
        }
        let idle_secs = spec
            .as_ref()
            .and_then(|spec| spec.idle_timeout_secs)
            .unwrap_or(DEFAULT_IDLE_TIMEOUT_SECS);
        let activity = latest_activity(store, task)?;
        if !is_stale(activity.timestamp, now, idle_secs) {
            continue;
        }
        if record_orphaned_hung(store, task_id, idle_secs, &activity)? {
            if let Some(spec) = spec.as_ref() {
                terminate_task_processes(spec.worker_pid, spec);
            }
            cleaned.push(task_id.to_string());
        }
    }
    Ok(cleaned)
}

pub(super) fn latest_activity(store: &Store, task: &Task) -> Result<TaskActivity> {
    let events = store.get_events(task.id.as_str())?;
    let progress_events = events.iter()
        .filter(|event| event.event_kind.is_liveness() && !is_idle_bookkeeping_event(event))
        .collect::<Vec<_>>();
    let last_event = progress_events.last();
    let mut timestamp = last_event.map(|event| event.timestamp).unwrap_or(task.created_at);
    let has_agent_bytes = agent_has_produced_bytes(task.id.as_str(), task.created_at);
    if let Some(bytes_at) = agent_bytes_mtime(task.id.as_str())
        && bytes_at > timestamp
    {
        timestamp = bytes_at;
    }
    Ok(TaskActivity {
        timestamp,
        event_count: progress_events.len() as u32,
        has_agent_bytes,
        detail: last_event.map(|event| event.detail.clone()),
    })
}

pub(super) fn is_stale(last_activity: DateTime<Local>, now: DateTime<Local>, idle_secs: u64) -> bool {
    (now - last_activity).num_seconds() >= idle_secs as i64
}

fn is_idle_bookkeeping_event(event: &TaskEvent) -> bool {
    // Aid's own setup (cargo target seed, setup scripts) is not agent output.
    // Keep excluding it: for buffered agents the first-token signal is bytes,
    // and Setup must not masquerade as either events or progress.
    if event.event_kind == EventKind::Setup {
        return true;
    }
    // PTY echo of aid's auto-nudge is often stored as bare Reasoning with no metadata.
    if event.event_kind == EventKind::Reasoning
        && event.detail.trim() == crate::unstick::default_nudge_message()
    {
        return true;
    }
    let Some(metadata) = event.metadata.as_ref() else {
        return false;
    };
    metadata.get("idle_warn").and_then(|value| value.as_bool()) == Some(true)
        || metadata.get("auto_escalated").and_then(|value| value.as_bool()) == Some(true)
        || metadata.get("acked_reply").and_then(|value| value.as_bool()) == Some(true)
        || metadata.get("source").and_then(|value| value.as_str()) == Some("unstick-auto")
}

/// Buffered agents (grok/agy) emit no progress events until exit; watch_buffered
/// appends stdout to the transcript as bytes arrive so the reaper can see them.
/// Bytes this run produced — not merely bytes that are lying there.
///
/// Presence alone was enough before, which let an `agent.log` left by an earlier attempt
/// on the same task id count as progress and silently move a dead spawn off the
/// first-token budget onto 2x idle. The file has to have been written after the task
/// started to say anything about this run.
fn agent_has_produced_bytes(task_id: &str, started_at: DateTime<Local>) -> bool {
    crate::paths::agent_has_produced_bytes(task_id, started_at.into())
}

fn agent_bytes_mtime(task_id: &str) -> Option<DateTime<Local>> {
    let mut best: Option<DateTime<Local>> = None;
    for path in crate::paths::agent_byte_paths(task_id) {
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if meta.len() == 0 {
            continue;
        }
        let Ok(modified) = meta.modified() else {
            continue;
        };
        let at = DateTime::<Local>::from(modified);
        best = Some(match best {
            Some(prev) if prev >= at => prev,
            _ => at,
        });
    }
    best
}

fn has_agent_progress(activity: &TaskActivity) -> bool {
    activity.event_count > 0 || activity.has_agent_bytes
}

fn record_orphaned_hung(
    store: &Store,
    task_id: &str,
    idle_secs: u64,
    activity: &TaskActivity,
) -> Result<bool> {
    let detail = format!("hung detected (orphaned supervisor): no output for {idle_secs}s");
    record_hung_detected_failure(store, task_id, idle_secs, activity, &detail)
}

pub(super) fn record_hung_detected_failure(
    store: &Store,
    task_id: &str,
    idle_secs: u64,
    activity: &TaskActivity,
    detail: &str,
) -> Result<bool> {
    if !super::record_failure(store, task_id, detail, detail)? {
        return Ok(false);
    }
    process_monitor::insert_hung_detected_events(
        store,
        &TaskId(task_id.to_string()),
        idle_secs,
        activity.event_count,
        activity.detail.as_deref(),
        false,
    )?;
    Ok(true)
}

/// Live worker with a wedged monitor: no agent progress for too long.
/// Zero agent output since spawn uses the first-token budget; silence after
/// progress uses 2× idle. Buffered agents signal progress via stdout bytes
/// (watch_buffered), not events — do not key first-token on event_count alone.
pub(super) fn cleanup_wedged_live_worker(
    store: &Store,
    task: &Task,
    spec: &BackgroundRunSpec,
    worker_pid: u32,
) -> Result<bool> {
    if task.status != TaskStatus::Running {
        return Ok(false);
    }
    let idle_secs = spec.idle_timeout_secs.unwrap_or(DEFAULT_IDLE_TIMEOUT_SECS);
    let activity = latest_activity(store, task)?;
    let saw_progress = has_agent_progress(&activity);
    let stale_after_secs = wedged_stale_after_secs(saw_progress, idle_secs, spec);
    if !is_stale(activity.timestamp, Local::now(), stale_after_secs) {
        return Ok(false);
    }
    let detail = wedged_failure_detail(saw_progress, stale_after_secs, idle_secs);
    terminate_task_processes(Some(worker_pid), spec);
    record_hung_detected_failure(store, task.id.as_str(), stale_after_secs, &activity, &detail)
}

fn wedged_stale_after_secs(saw_progress: bool, idle_secs: u64, spec: &BackgroundRunSpec) -> u64 {
    if saw_progress {
        idle_secs.saturating_mul(LIVE_WORKER_IDLE_MARGIN)
    } else {
        crate::timeout_policy::TimeoutPolicy::from_env(spec.env.as_ref())
            .first_token
            .as_secs()
    }
}

fn wedged_failure_detail(saw_progress: bool, stale_after_secs: u64, idle_secs: u64) -> String {
    if saw_progress {
        format!(
            "hung detected (monitor wedged): no events for {stale_after_secs}s \
             (idle timeout {idle_secs}s, margin {LIVE_WORKER_IDLE_MARGIN}x)"
        )
    } else {
        format!(
            "hung detected (monitor wedged): no agent output since spawn for {stale_after_secs}s \
             (first-token timeout)"
        )
    }
}

pub(super) struct TaskActivity {
    pub(super) timestamp: DateTime<Local>,
    pub(super) event_count: u32,
    pub(super) has_agent_bytes: bool,
    pub(super) detail: Option<String>,
}

#[cfg(test)]
#[path = "background_orphan_tests.rs"]
mod tests;
