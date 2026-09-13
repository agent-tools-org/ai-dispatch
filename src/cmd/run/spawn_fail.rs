// Terminalize tasks whose agent process never started.
// Exports: fail_task_on_agent_spawn, insert_phase_error_event.
// Deps: Store, task lifecycle, event types.
use chrono::Local;
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
        crate::paths::stderr_path(task_id.as_str()),
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
