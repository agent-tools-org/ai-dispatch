// Terminalize tasks whose agent process never started.
// Exports: fail_task_on_agent_spawn, insert_phase_error_event.
// Deps: Store, task lifecycle, event types.
use chrono::Local;
use super::{FailureContext, FAST_FAIL_SNAPSHOT_MS};
use crate::store::{Store, TaskCompletionUpdate};
use crate::types::{EventKind, TaskEvent, TaskId, TaskStatus};

pub(crate) fn insert_phase_error_event(
    store: &Store,
    task_id: &TaskId,
    phase: &str,
    error: &str,
    stderr: Option<&str>,
) {
    let mut detail = format!("Failed during {phase}: {error}");
    if let Some(stderr) = stderr.filter(|stderr| !stderr.is_empty()) {
        detail.push_str("\nStderr: ");
        detail.push_str(stderr);
    }
    let _ = store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Error,
        detail,
        metadata: None,
    });
}

/// Spawn never started — no worker/watchdog will reclaim the row. Terminalize now.
pub(crate) fn fail_task_on_agent_spawn(
    store: &Store,
    task_id: &TaskId,
    err: &anyhow::Error,
    stderr: Option<&str>,
) {
    let mut detail = format!("Failed during agent spawn: {err}");
    if let Some(stderr) = stderr.filter(|stderr| !stderr.is_empty()) {
        detail.push_str("\nStderr: ");
        detail.push_str(stderr);
    }
    let _ = std::fs::write(
        crate::support::paths::stderr_path(task_id.as_str()),
        format!("{detail}\n"),
    );
    let _ = crate::task_lifecycle::complete_task_atomic(
        store,
        TaskCompletionUpdate {
            id: task_id.as_str(),
            status: TaskStatus::Failed,
            tokens: None,
            duration_ms: 0,
            observed_model: None,
            attribution_source: None,
            cost_usd: None,
            exit_code: None,
        },
        &TaskEvent {
            task_id: task_id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Error,
            detail,
            metadata: None,
        },
    );
}

pub(super) fn insert_fast_fail_snapshot_event(store: &Store, task_id: &TaskId, duration_ms: i64, exit_code: Option<i32>, context: &FailureContext) {
    if duration_ms >= FAST_FAIL_SNAPSHOT_MS || !matches!(exit_code, Some(code) if code != 0) {
        return;
    }
    let detail = format!(
        "Failure context: working directory: {}; agent binary: {}; worktree path: {}; worktree created: {}",
        context
            .working_dir
            .as_deref()
            .unwrap_or("(inherit current process directory)"),
        if context.binary_path.is_empty() {
            "(unknown)"
        } else {
            context.binary_path.as_str()
        },
        context.worktree_path.as_deref().unwrap_or("(none)"),
        context.worktree_created,
    );
    let _ = store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Error,
        detail,
        metadata: None,
    });
}
