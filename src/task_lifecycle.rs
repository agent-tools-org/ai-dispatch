// Task status intent layer for lifecycle side effects.
// Exports failure transition helpers that keep Store mutations pure.
// Deps: Store, failure salvage, and task status/event payload types.

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
    store.update_task_status(task_id, TaskStatus::Stopped)
}

pub(crate) fn restore_after_merge_failure(
    store: &Store,
    task_id: &str,
    status: TaskStatus,
) -> Result<bool> {
    store.update_task_status(task_id, status)
}

pub(crate) fn rescue_to_done(store: &Store, task_id: &TaskId) -> Result<bool> {
    store.rescue_task_to_done(task_id.as_str())
}

pub(crate) fn mark_failed(store: &Store, task_id: &TaskId) -> Result<bool> {
    let changed = store.update_task_status(task_id.as_str(), TaskStatus::Failed)?;
    if !changed {
        return Ok(false);
    }
    salvage_failed_task(store, task_id);
    Ok(true)
}

pub(crate) fn fail_completed_verify_gate(store: &Store, task_id: &TaskId) -> Result<bool> {
    let changed = store.fail_completed_verify_gate(task_id.as_str())?;
    salvage_failed_id(store, task_id.as_str(), changed);
    Ok(changed)
}

pub(crate) fn fail_if_running(store: &Store, task_id: &str) -> Result<bool> {
    let failed = store.fail_if_running(task_id)?;
    salvage_failed_id(store, task_id, failed);
    Ok(failed)
}

pub(crate) fn fail_active_execution(store: &Store, task_id: &str) -> Result<bool> {
    let failed = store.fail_active_execution(task_id)?;
    salvage_failed_id(store, task_id, failed);
    Ok(failed)
}

pub(crate) fn fail_pending_with_reason(
    store: &Store,
    task_id: &str,
    pending_reason: PendingReason,
) -> Result<bool> {
    let failed = store.fail_pending_with_reason(task_id, pending_reason)?;
    salvage_failed_id(store, task_id, failed);
    Ok(failed)
}

pub(crate) fn fail_waiting_with_reason(
    store: &Store,
    task_id: &str,
    detail: &str,
) -> Result<bool> {
    let failed = store.fail_waiting_with_reason(task_id, detail)?;
    salvage_failed_id(store, task_id, failed);
    Ok(failed)
}

pub(crate) fn update_task_completion(
    store: &Store,
    payload: TaskCompletionUpdate<'_>,
) -> Result<()> {
    let task_id = TaskId(payload.id.to_string());
    let status = payload.status;
    let changed = store.update_task_completion(payload)?;
    salvage_failed_transition(store, &task_id, status, changed);
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
    salvage_failed_transition(store, &task_id, status, changed);
    Ok(())
}

fn salvage_failed_id(store: &Store, task_id: &str, changed: bool) {
    if !changed {
        return;
    }
    salvage_failed_task(store, &TaskId(task_id.to_string()));
}

fn salvage_failed_transition(
    store: &Store,
    task_id: &TaskId,
    status: TaskStatus,
    changed: bool,
) {
    if changed && status == TaskStatus::Failed {
        salvage_failed_task(store, task_id);
    }
}

fn salvage_failed_task(store: &Store, task_id: &TaskId) {
    crate::failure_salvage::salvage_failed_task(store, task_id);
}
