// Task status intent layer for lifecycle side effects.
// Exports failure transition helpers that keep Store mutations pure.
// Deps: Store, failure salvage, and task status/event payload types.

mod failure;

use anyhow::Result;

use crate::store::{Store, TaskCompletionUpdate};
use crate::types::{ACTIVE_TASK_STATUSES, TaskEvent, TaskId, TaskStatus};

pub(crate) fn mark_running(store: &Store, task_id: &TaskId) -> Result<bool> {
    store.settle_task_status(task_id.as_str(), TaskStatus::Running, Some(&ACTIVE_TASK_STATUSES), None, None)
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
) -> Result<bool> {
    record_completion(store, payload, None)
}

pub(crate) fn complete_task_atomic(
    store: &Store,
    payload: TaskCompletionUpdate<'_>,
    event: &TaskEvent,
) -> Result<bool> {
    record_completion(store, payload, Some(event))
}

fn record_completion(
    store: &Store,
    payload: TaskCompletionUpdate<'_>,
    event: Option<&TaskEvent>,
) -> Result<bool> {
    let task_id = TaskId(payload.id.to_string());
    let status = payload.status;
    let changed = store.record_agent_completion(payload, event)?;
    salvage_failed_transition(store, &task_id, status, changed);
    Ok(changed)
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

pub(crate) fn salvage_failed_task(store: &Store, task_id: &TaskId) {
    failure::record_unclassified(store, task_id);
    if let Ok(Some(task)) = store.get_task(task_id.as_str())
        && let Some(message) = crate::routing::capacity::read(store, task_id, task.agent)
    {
        let model = task.requested_model.as_deref().or(task.observed_model.as_deref());
        crate::routing::rate_limit::mark_rate_limited_for_model(
            &task.agent, task.custom_agent_name.as_deref(), model, &message,
        );
    }
    crate::task::failure_salvage::salvage_failed_task(store, task_id);
}
