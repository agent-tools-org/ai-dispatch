// Task status intent layer for lifecycle side effects.
// Exports transition helpers that keep Store mutations pure; every terminal
// transition funnels through after_transition (failure salvage). Backup runs
// from settle_now only for transitions nothing follows (stop, reaper); process
// completions are backed up by the post-run lifecycle once they have settled.
// Deps: Store, failure salvage, backup, and task status/event payload types.

use anyhow::Result;

use crate::store::{Store, TaskCompletionUpdate};
use crate::types::{PendingReason, TaskEvent, TaskId, TaskStatus};

pub(crate) fn mark_running(store: &Store, task_id: &TaskId) -> Result<bool> {
    store.update_task_status(task_id.as_str(), TaskStatus::Running)
}

pub(crate) fn mark_awaiting_input(store: &Store, task_id: &TaskId) -> Result<bool> {
    store.update_task_status(task_id.as_str(), TaskStatus::AwaitingInput)
}

pub(crate) fn mark_stalled(store: &Store, task_id: &str) -> Result<bool> {
    store.update_task_status(task_id, TaskStatus::Stalled)
}

pub(crate) fn mark_merged(store: &Store, task_id: &str) -> Result<bool> {
    store.update_task_status(task_id, TaskStatus::Merged)
}

pub(crate) fn mark_skipped(store: &Store, task_id: &str) -> Result<bool> {
    store.update_task_status(task_id, TaskStatus::Skipped)
}

pub(crate) fn mark_stopped(store: &Store, task_id: &str) -> Result<bool> {
    let changed = store.update_task_status(task_id, TaskStatus::Stopped)?;
    settle_now(store, task_id, TaskStatus::Stopped, changed);
    Ok(changed)
}

pub(crate) fn restore_after_merge_failure(
    store: &Store,
    task_id: &str,
    status: TaskStatus,
) -> Result<bool> {
    store.update_task_status(task_id, status)
}

pub(crate) fn rescue_to_done(store: &Store, task_id: &TaskId) -> Result<bool> {
    let changed = store.rescue_task_to_done(task_id.as_str())?;
    after_transition(store, task_id.as_str(), TaskStatus::Done, changed);
    Ok(changed)
}

pub(crate) fn mark_failed(store: &Store, task_id: &TaskId) -> Result<bool> {
    let changed = store.update_task_status(task_id.as_str(), TaskStatus::Failed)?;
    after_transition(store, task_id.as_str(), TaskStatus::Failed, changed);
    Ok(changed)
}

pub(crate) fn fail_completed_verify_gate(store: &Store, task_id: &TaskId) -> Result<bool> {
    let changed = store.fail_completed_verify_gate(task_id.as_str())?;
    after_transition(store, task_id.as_str(), TaskStatus::Failed, changed);
    Ok(changed)
}

pub(crate) fn fail_if_running(store: &Store, task_id: &str) -> Result<bool> {
    let failed = store.fail_if_running(task_id)?;
    settle_now(store, task_id, TaskStatus::Failed, failed);
    Ok(failed)
}

pub(crate) fn fail_active_execution(store: &Store, task_id: &str) -> Result<bool> {
    let failed = store.fail_active_execution(task_id)?;
    settle_now(store, task_id, TaskStatus::Failed, failed);
    Ok(failed)
}

pub(crate) fn fail_pending_with_reason(
    store: &Store,
    task_id: &str,
    pending_reason: PendingReason,
) -> Result<bool> {
    let failed = store.fail_pending_with_reason(task_id, pending_reason)?;
    settle_now(store, task_id, TaskStatus::Failed, failed);
    Ok(failed)
}

pub(crate) fn fail_waiting_with_reason(
    store: &Store,
    task_id: &str,
    detail: &str,
) -> Result<bool> {
    let failed = store.fail_waiting_with_reason(task_id, detail)?;
    settle_now(store, task_id, TaskStatus::Failed, failed);
    Ok(failed)
}

pub(crate) fn update_task_completion(
    store: &Store,
    payload: TaskCompletionUpdate<'_>,
) -> Result<()> {
    let task_id = TaskId(payload.id.to_string());
    let status = payload.status;
    let changed = store.update_task_completion(payload)?;
    after_transition(store, task_id.as_str(), status, changed);
    Ok(())
}

pub(crate) fn complete_task_atomic(
    store: &Store,
    payload: TaskCompletionUpdate<'_>,
    event: &TaskEvent,
) -> Result<()> {
    let task_id = TaskId(payload.id.to_string());
    let status = payload.status;
    let changed = store.complete_task_atomic(payload, event)?;
    after_transition(store, task_id.as_str(), status, changed);
    Ok(())
}

/// The one place terminal transitions fan out to side effects. `changed`
/// false means the store refused the transition, so nothing happened.
fn after_transition(store: &Store, task_id: &str, status: TaskStatus, changed: bool) {
    if !changed {
        return;
    }
    if status == TaskStatus::Failed {
        crate::failure_salvage::salvage_failed_task(store, &TaskId(task_id.to_string()));
    }
}

/// For transitions that end the task right here (stop, reaper): the status is
/// already settled, so back up now. Process completions skip this and are
/// backed up by `post_run_lifecycle` after verify and result persistence.
fn settle_now(store: &Store, task_id: &str, status: TaskStatus, changed: bool) {
    after_transition(store, task_id, status, changed);
    if changed {
        crate::backup::on_terminal(store, task_id, status);
    }
}
