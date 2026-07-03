// Task status intent layer for lifecycle side effects.
// Exports failure transition helpers that keep Store mutations pure.
// Deps: Store, failure salvage, and task status/event payload types.

use anyhow::Result;

use crate::store::{Store, TaskCompletionUpdate};
use crate::types::{PendingReason, TaskEvent, TaskId, TaskStatus};

pub(crate) fn mark_failed(store: &Store, task_id: &TaskId) -> Result<()> {
    store.update_task_status(task_id.as_str(), TaskStatus::Failed)?;
    salvage_failed_task(store, task_id);
    Ok(())
}

pub(crate) fn fail_if_running(store: &Store, task_id: &str) -> Result<bool> {
    let failed = store.fail_if_running(task_id)?;
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
