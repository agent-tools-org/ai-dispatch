// Typed action failures and shared response mapping for web mutations.
// Exports: ActionError, action_result, and action guard helpers.
// Deps: axum JSON responses, Store task state, and command validation.

use axum::Json;
use axum::http::StatusCode;

use crate::store::Store;
use crate::types::TaskStatus;

use super::api_types::ActionResponse;

pub(crate) enum ActionError {
    NotFound(String),
    Conflict(String),
    Internal(anyhow::Error),
}

pub(crate) type ActionResponseResult = (StatusCode, Json<ActionResponse>);

pub(crate) fn internal(error: anyhow::Error) -> ActionError {
    ActionError::Internal(error)
}

pub(crate) fn not_found(message: impl Into<String>) -> ActionError {
    ActionError::NotFound(message.into())
}

pub(crate) fn action_result(result: Result<(), ActionError>) -> ActionResponseResult {
    match result {
        Ok(()) => action_ok(None),
        Err(error) => action_error(error),
    }
}

pub(crate) fn action_ok(new_task_id: Option<String>) -> ActionResponseResult {
    (
        StatusCode::OK,
        Json(ActionResponse { ok: true, new_task_id, error: None }),
    )
}

pub(crate) fn action_error(error: ActionError) -> ActionResponseResult {
    let (status, message) = match error {
        ActionError::NotFound(message) => (StatusCode::NOT_FOUND, message),
        ActionError::Conflict(message) => (StatusCode::CONFLICT, message),
        ActionError::Internal(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    };
    (
        status,
        Json(ActionResponse { ok: false, new_task_id: None, error: Some(message) }),
    )
}

pub(crate) fn ensure_merge_allowed(store: &Store, task_id: &str) -> Result<(), ActionError> {
    let task = store
        .get_task(task_id)
        .map_err(internal)?
        .ok_or_else(|| not_found(format!("Task {task_id} not found")))?;
    crate::cmd::merge::validate_merge_outcome(&task, task.outcome(), false)
        .map_err(|error| ActionError::Conflict(error.to_string()))
}

pub(crate) fn ensure_exists(store: &Store, task_id: &str) -> Result<(), ActionError> {
    store
        .get_task(task_id)
        .map_err(internal)?
        .ok_or_else(|| not_found(format!("Task {task_id} not found")))
        .map(|_| ())
}

pub(crate) fn ensure_terminal(store: &Store, task_id: &str) -> Result<(), ActionError> {
    let task = store
        .get_task(task_id)
        .map_err(internal)?
        .ok_or_else(|| not_found(format!("Task {task_id} not found")))?;
    if task.status.is_terminal() {
        return Ok(());
    }
    Err(ActionError::Conflict(format!(
        "Task {task_id} is not complete; principal acceptance is not allowed"
    )))
}

pub(crate) fn ensure_replyable(store: &Store, task_id: &str) -> Result<(), ActionError> {
    let task = store
        .get_task(task_id)
        .map_err(internal)?
        .ok_or_else(|| not_found(format!("Task {task_id} not found")))?;
    if matches!(task.status, TaskStatus::Running | TaskStatus::AwaitingInput | TaskStatus::Stalled) {
        return Ok(());
    }
    Err(ActionError::Conflict(format!(
        "Task {task_id} is {} — can only reply to running tasks",
        task.status.label()
    )))
}
