// Web API handlers for task, event, output, action, and usage endpoints.
// Exports: additive `/api/` handlers and shared task response enrichment.
// Deps: axum, Store batch queries, task actions, and API DTOs.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use crate::cmd;
use crate::store::Store;
use crate::task_actions;
use crate::task_view;
use crate::types::{Task, TaskFilter};

pub(crate) use super::api_types::{
    ActionResponse, AgentUsageResponse, DiffResponse, MessageRequest, ResultResponse,
    RetryRequest, TaskEventResponse, TaskListParams, TaskOutputResponse, TaskResponse,
    UsageResponse,
};
use super::api_types::TaskEnrichment;
use super::diff::has_non_empty_diff;

pub async fn list_tasks(
    Query(params): Query<TaskListParams>,
    State(store): State<Arc<Store>>,
) -> Result<Json<Vec<TaskResponse>>, StatusCode> {
    let filter = parse_filter(params.filter.as_deref()).ok_or(StatusCode::BAD_REQUEST)?;
    let tasks = store.list_tasks(filter).map_err(internal_error)?;
    Ok(Json(enrich_tasks(&store, tasks).map_err(internal_error)?))
}

pub async fn get_task(
    Path(id): Path<String>,
    State(store): State<Arc<Store>>,
) -> Result<Json<TaskResponse>, StatusCode> {
    let task = store.get_task(&id).map_err(internal_error)?.ok_or(StatusCode::NOT_FOUND)?;
    let response = enrich_tasks(&store, vec![task])
        .map_err(internal_error)?
        .into_iter()
        .next()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(response))
}

pub async fn get_task_events(
    Path(id): Path<String>,
    State(store): State<Arc<Store>>,
) -> Result<Json<Vec<TaskEventResponse>>, StatusCode> {
    ensure_task_exists(&store, &id)?;
    let events = store.get_events(&id).map_err(internal_error)?;
    Ok(Json(events.into_iter().map(TaskEventResponse::from).collect()))
}

pub async fn get_task_output(
    Path(id): Path<String>,
    State(store): State<Arc<Store>>,
) -> Result<Json<TaskOutputResponse>, StatusCode> {
    let task = store.get_task(&id).map_err(internal_error)?.ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(TaskOutputResponse { output: task_view::read_output(&task) }))
}

pub async fn get_task_result(
    Path(id): Path<String>,
    State(store): State<Arc<Store>>,
) -> Result<Json<ResultResponse>, StatusCode> {
    ensure_task_exists(&store, &id)?;
    let path = crate::paths::task_dir(&id).join("result.md");
    let result = std::fs::read_to_string(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    })?;
    Ok(Json(ResultResponse { result }))
}

pub async fn get_usage(State(store): State<Arc<Store>>) -> Result<Json<UsageResponse>, StatusCode> {
    let success_rates = store.agent_success_rates().map_err(internal_error)?;
    let avg_costs: HashMap<_, _> = store.agent_avg_costs().map_err(internal_error)?.into_iter().collect();
    let agents = success_rates
        .into_iter()
        .map(|(agent, success_rate, task_count)| AgentUsageResponse {
            agent: agent.as_str().to_string(),
            success_rate: Some(success_rate),
            task_count,
            avg_cost: avg_costs.get(&agent).copied(),
        })
        .collect();
    Ok(Json(UsageResponse { agents }))
}

pub async fn stop_task(Path(id): Path<String>, State(store): State<Arc<Store>>) -> impl IntoResponse {
    match store.get_task(&id) {
        Ok(Some(task)) if task.status.is_terminal() => action_ok(None),
        Ok(Some(_)) => action_result(task_actions::stop(&store, &id)),
        Ok(None) => action_error(anyhow::anyhow!("Task {id} not found")),
        Err(error) => action_error(error),
    }
}

pub async fn retry_task(
    Path(id): Path<String>,
    State(store): State<Arc<Store>>,
    Json(request): Json<RetryRequest>,
) -> impl IntoResponse {
    let args = task_actions::RetryArgs {
        task_id: id,
        feedback: request.feedback,
        feedback_file: None,
        agent: None,
        model: None,
        idle_timeout_secs: None,
        dir: None,
        reset: false,
        bg: false,
    };
    match task_actions::retry(store, args).await {
        Ok(new_task_id) => action_ok(Some(new_task_id.to_string())),
        Err(error) => action_error(error),
    }
}

pub async fn merge_task(Path(id): Path<String>, State(store): State<Arc<Store>>) -> impl IntoResponse {
    action_result(task_actions::merge(
        store,
        task_actions::MergeArgs {
            task_id: Some(&id),
            group: None,
            approve: true,
            check: false,
            force: false,
            target: None,
            lanes: false,
        },
    ))
}

pub async fn steer_task(
    Path(id): Path<String>,
    State(store): State<Arc<Store>>,
    Json(request): Json<MessageRequest>,
) -> impl IntoResponse {
    action_result(cmd::steer::run(&store, &id, &request.message))
}

pub async fn respond_task(
    Path(id): Path<String>,
    State(store): State<Arc<Store>>,
    Json(request): Json<MessageRequest>,
) -> impl IntoResponse {
    action_result(cmd::respond::run(&store, &id, Some(&request.message), None))
}

pub async fn accept_task(Path(id): Path<String>, State(store): State<Arc<Store>>) -> impl IntoResponse {
    action_result(crate::artifact_custody::accept(&store, &id, &local_principal()))
}

pub async fn reject_task(Path(id): Path<String>, State(store): State<Arc<Store>>) -> impl IntoResponse {
    action_result(crate::artifact_custody::reject(&store, &id, &local_principal()))
}

pub async fn get_task_diff(Path(id): Path<String>, State(store): State<Arc<Store>>) -> impl IntoResponse {
    match task_view::diff_text(&store, &id) {
        Ok(diff) if diff_unavailable(&diff) => StatusCode::NOT_FOUND.into_response(),
        Ok(diff) => (StatusCode::OK, Json(DiffResponse { diff })).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

pub(crate) fn enrich_tasks(store: &Store, tasks: Vec<Task>) -> anyhow::Result<Vec<TaskResponse>> {
    let ids: Vec<&str> = tasks.iter().map(|task| task.id.as_str()).collect();
    let memory_mb: HashMap<_, _> = tasks
        .iter()
        .map(|task| (task.id.as_str().to_string(), task_memory_mb(task)))
        .collect();
    let started_at = store.started_at_batch(&ids)?;
    let profiles = store.get_task_profiles_batch(&ids)?;
    let milestones = store.latest_milestones_batch(&ids)?;
    let mut errors = store.latest_errors_batch(&ids)?;
    for (id, error) in store.latest_errors_batch_unfiltered(&ids)? {
        errors.entry(id).or_insert(error);
    }
    let awaiting = store.latest_awaiting_reasons_batch(&ids)?;
    let events = store.latest_events_three_batch(&ids)?;
    Ok(tasks
        .into_iter()
        .map(|task| {
            let id = task.id.as_str().to_string();
            let has_diff = has_non_empty_diff(&task);
            TaskResponse::from_task(task, TaskEnrichment {
                started_at: started_at.get(&id).cloned(),
                memory_mb: memory_mb.get(&id).copied().flatten(),
                profile: profiles.get(&id).copied().unwrap_or_default(),
                latest_milestone: milestones.get(&id).cloned(),
                latest_error: errors.get(&id).cloned(),
                awaiting_reason: awaiting.get(&id).cloned(),
                latest_events: events
                    .get(&id)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(TaskEventResponse::from)
                    .collect(),
                has_diff,
            })
        })
        .collect())
}

pub(crate) fn task_memory_mb(task: &Task) -> Option<i64> {
    if !matches!(task.status, crate::types::TaskStatus::Running | crate::types::TaskStatus::AwaitingInput) {
        return None;
    }
    let worker_pid = crate::background::load_worker_pid(task.id.as_str()).ok().flatten()?;
    crate::tui::metrics::get_process_metrics(worker_pid)
        .map(|metrics| metrics.memory_mb.round() as i64)
}

fn parse_filter(filter: Option<&str>) -> Option<TaskFilter> {
    match filter.unwrap_or("today") {
        "all" => Some(TaskFilter::All),
        "running" => Some(TaskFilter::Running),
        "today" => Some(TaskFilter::Today),
        _ => None,
    }
}

fn ensure_task_exists(store: &Store, id: &str) -> Result<(), StatusCode> {
    store.get_task(id).map_err(internal_error)?.ok_or(StatusCode::NOT_FOUND).map(|_| ())
}

fn local_principal() -> String {
    std::env::var("USER")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "local-principal".to_string())
}

fn action_result(result: anyhow::Result<()>) -> (StatusCode, Json<ActionResponse>) {
    match result {
        Ok(()) => action_ok(None),
        Err(error) => action_error(error),
    }
}

fn action_ok(new_task_id: Option<String>) -> (StatusCode, Json<ActionResponse>) {
    (StatusCode::OK, Json(ActionResponse { ok: true, new_task_id, error: None }))
}

pub(crate) fn internal_error(_: anyhow::Error) -> StatusCode {
    StatusCode::INTERNAL_SERVER_ERROR
}

fn action_error(error: anyhow::Error) -> (StatusCode, Json<ActionResponse>) {
    let message = error.to_string();
    let status = if message.contains("not found") {
        StatusCode::NOT_FOUND
    } else if message.contains("can only")
        || message.contains("not complete")
        || message.contains("acceptance is not allowed")
    {
        StatusCode::CONFLICT
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    (status, Json(ActionResponse { ok: false, new_task_id: None, error: Some(message) }))
}

fn diff_unavailable(diff: &str) -> bool {
    diff.contains("(worktree removed or diff unavailable)")
        || diff.contains("(no worktree diff or output file available)")
        || diff.contains("(in-place edit — no uncommitted changes detected, may already be committed)")
}
