// Orphaned background-task staleness cleanup.
// Exports cleanup_orphaned_idle_tasks for background zombie reconciliation.
// Deps: background specs, idle timeout defaults, store events, and task types.

use anyhow::Result;
use chrono::{DateTime, Local};

use super::background_kill::terminate_task_processes;
use super::background_spec::{load_spec_if_exists, BackgroundRunSpec};
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
        let Some(spec) = load_spec_if_exists(task_id)? else {
            continue;
        };
        if spec.worker_pid.is_some_and(is_process_alive) {
            continue;
        }
        let idle_secs = spec.idle_timeout_secs.unwrap_or(DEFAULT_IDLE_TIMEOUT_SECS);
        let activity = latest_activity(store, task)?;
        if !is_stale(activity.timestamp, now, idle_secs) {
            continue;
        }
        if record_orphaned_hung(store, task_id, idle_secs, &activity)? {
            terminate_task_processes(spec.worker_pid, &spec);
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
    Ok(TaskActivity {
        timestamp: last_event.map(|event| event.timestamp).unwrap_or(task.created_at),
        event_count: progress_events.len() as u32,
        detail: last_event.map(|event| event.detail.clone()),
    })
}

pub(super) fn is_stale(last_activity: DateTime<Local>, now: DateTime<Local>, idle_secs: u64) -> bool {
    (now - last_activity).num_seconds() >= idle_secs as i64
}

fn is_idle_bookkeeping_event(event: &TaskEvent) -> bool {
    // Aid's own setup (cargo target seed, setup scripts) is not agent output.
    // t-764b2a1d already had a Setup event before any agent bytes — counting it
    // as liveness kept the wedged reaper on 2× idle instead of first-token.
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
    if !super::record_failure(store, task_id, &detail, &detail)? {
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

/// Live worker with a wedged monitor: no progress events for too long.
/// Zero events since spawn uses the first-token budget; silence after progress
/// uses 2× idle (the buffered path never builds MonitorState).
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
    let stale_after_secs = wedged_stale_after_secs(activity.event_count, idle_secs, spec);
    if !is_stale(activity.timestamp, Local::now(), stale_after_secs) {
        return Ok(false);
    }
    let detail = wedged_failure_detail(activity.event_count, stale_after_secs, idle_secs);
    terminate_task_processes(Some(worker_pid), spec);
    record_hung_detected_failure(store, task.id.as_str(), stale_after_secs, &activity, &detail)
}

fn wedged_stale_after_secs(event_count: u32, idle_secs: u64, spec: &BackgroundRunSpec) -> u64 {
    if event_count == 0 {
        crate::timeout_policy::TimeoutPolicy::from_env(spec.env.as_ref())
            .first_token
            .as_secs()
    } else {
        idle_secs.saturating_mul(LIVE_WORKER_IDLE_MARGIN)
    }
}

fn wedged_failure_detail(event_count: u32, stale_after_secs: u64, idle_secs: u64) -> String {
    if event_count == 0 {
        format!(
            "hung detected (monitor wedged): no events since spawn for {stale_after_secs}s \
             (first-token timeout)"
        )
    } else {
        format!(
            "hung detected (monitor wedged): no events for {stale_after_secs}s \
             (idle timeout {idle_secs}s, margin {LIVE_WORKER_IDLE_MARGIN}x)"
        )
    }
}

pub(super) struct TaskActivity {
    pub(super) timestamp: DateTime<Local>,
    pub(super) event_count: u32,
    pub(super) detail: Option<String>,
}

#[cfg(test)]
#[path = "background_orphan_tests.rs"]
mod tests;
