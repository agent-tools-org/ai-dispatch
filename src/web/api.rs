// Web API handlers for task, event, output, and usage JSON endpoints.
// Exports: axum handlers and response DTOs for the web UI.
// Deps: axum, serde, crate::store, crate::types.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::store::Store;
use crate::types::{Task, TaskEvent, TaskFilter};

#[derive(Debug, Deserialize)]
pub struct TaskListParams { pub filter: Option<String> }

#[derive(Debug, Serialize)]
pub struct TaskResponse {
    pub id: String,
    pub agent: String,
    pub custom_agent_name: Option<String>,
    pub prompt: String,
    pub resolved_prompt: Option<String>,
    pub status: String,
    pub parent_task_id: Option<String>,
    pub workgroup_id: Option<String>,
    pub caller_kind: Option<String>,
    pub caller_session_id: Option<String>,
    pub agent_session_id: Option<String>,
    pub repo_path: Option<String>,
    pub worktree_path: Option<String>,
    pub worktree_branch: Option<String>,
    pub log_path: Option<String>,
    pub output_path: Option<String>,
    pub tokens: Option<i64>,
    pub prompt_tokens: Option<i64>,
    pub duration_ms: Option<i64>,
    pub model: Option<String>,
    pub cost_usd: Option<f64>,
    pub exit_code: Option<i32>,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub verify: Option<String>,
    pub verify_status: String,
    pub read_only: bool,
    pub budget: bool,
    pub latest_milestone: Option<String>,
    pub latest_error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TaskEventResponse {
    pub task_id: String,
    pub timestamp: String,
    pub event_kind: String,
    pub detail: String,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct TaskOutputResponse { pub output: String }

#[derive(Debug, Serialize)]
pub struct AgentUsageResponse {
    pub agent: String,
    pub success_rate: Option<f64>,
    pub task_count: usize,
    pub avg_cost: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct UsageResponse { pub agents: Vec<AgentUsageResponse> }

#[derive(Debug, Serialize)]
pub struct ActionResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DiffResponse { pub diff: String }

#[derive(Debug, Deserialize)]
pub struct RetryRequest { pub feedback: Option<String> }

impl TaskResponse {
    pub(crate) fn from_task(
        task: Task,
        latest_milestone: Option<String>,
        latest_error: Option<String>,
    ) -> Self {
        Self {
            id: task.id.to_string(),
            agent: task.agent.as_str().to_string(),
            custom_agent_name: task.custom_agent_name,
            prompt: task.prompt,
            resolved_prompt: task.resolved_prompt,
            status: task.status.as_str().to_string(),
            parent_task_id: task.parent_task_id,
            workgroup_id: task.workgroup_id,
            caller_kind: task.caller_kind,
            caller_session_id: task.caller_session_id,
            agent_session_id: task.agent_session_id,
            repo_path: task.repo_path,
            worktree_path: task.worktree_path,
            worktree_branch: task.worktree_branch,
            log_path: task.log_path,
            output_path: task.output_path,
            tokens: task.tokens,
            prompt_tokens: task.prompt_tokens,
            duration_ms: task.duration_ms,
            model: task.model,
            cost_usd: task.cost_usd,
            exit_code: task.exit_code,
            created_at: task.created_at.to_rfc3339(),
            completed_at: task.completed_at.map(|value| value.to_rfc3339()),
            verify: task.verify,
            verify_status: task.verify_status.as_str().to_string(),
            read_only: task.read_only,
            budget: task.budget,
            latest_milestone,
            latest_error,
        }
    }
}

impl From<TaskEvent> for TaskEventResponse {
    fn from(event: TaskEvent) -> Self {
        Self {
            task_id: event.task_id.to_string(),
            timestamp: event.timestamp.to_rfc3339(),
            event_kind: event.event_kind.as_str().to_string(),
            detail: event.detail,
            metadata: event.metadata,
        }
    }
}

pub async fn list_tasks(
    Query(params): Query<TaskListParams>,
    State(store): State<Arc<Store>>,
) -> Result<Json<Vec<TaskResponse>>, StatusCode> {
    let filter = parse_filter(params.filter.as_deref()).ok_or(StatusCode::BAD_REQUEST)?;
    let tasks = store.list_tasks(filter).map_err(internal_error)?;
    let task_ids: Vec<&str> = tasks.iter().map(|task| task.id.as_str()).collect();
    let milestones = store.latest_milestones_batch(&task_ids).map_err(internal_error)?;
    let response = tasks
        .into_iter()
        .map(|task| {
            let milestone = milestones.get(task.id.as_str()).cloned();
            let error = store.latest_error(task.id.as_str());
            TaskResponse::from_task(task, milestone, error)
        })
        .collect();
    Ok(Json(response))
}

pub async fn get_task(
    Path(id): Path<String>,
    State(store): State<Arc<Store>>,
) -> Result<Json<TaskResponse>, StatusCode> {
    let task = store.get_task(&id).map_err(internal_error)?.ok_or(StatusCode::NOT_FOUND)?;
    let milestone = store.latest_milestone(&id).map_err(internal_error)?;
    Ok(Json(TaskResponse::from_task(task, milestone, store.latest_error(&id))))
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
    Ok(Json(TaskOutputResponse { output: read_task_output(&task) }))
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
    match crate::cmd::stop::stop(&store, &id) {
        Ok(()) => (StatusCode::OK, Json(ActionResponse { ok: true, new_task_id: None, error: None })).into_response(),
        Err(error) => action_error(error).into_response(),
    }
}

pub async fn retry_task(
    Path(id): Path<String>,
    State(store): State<Arc<Store>>,
    Json(request): Json<RetryRequest>,
) -> impl IntoResponse {
    match crate::cmd::retry::run(store, crate::cmd::retry::RetryArgs {
        task_id: id,
        feedback: request.feedback.unwrap_or_default(),
        agent: None,
        dir: None,
        reset: false,
    }).await {
        Ok(new_task_id) => (StatusCode::OK, Json(ActionResponse {
            ok: true,
            new_task_id: Some(new_task_id.to_string()),
            error: None,
        })).into_response(),
        Err(error) => action_error(error).into_response(),
    }
}

pub async fn merge_task(Path(id): Path<String>, State(store): State<Arc<Store>>) -> impl IntoResponse {
    match crate::cmd::merge::run(store, Some(&id), None, true, false, None) {
        Ok(()) => (StatusCode::OK, Json(ActionResponse { ok: true, new_task_id: None, error: None })).into_response(),
        Err(error) => action_error(error).into_response(),
    }
}

pub async fn get_task_diff(Path(id): Path<String>, State(store): State<Arc<Store>>) -> impl IntoResponse {
    match crate::cmd::show::diff_text(&store, &id) {
        Ok(diff) if diff_unavailable(&diff) => StatusCode::NOT_FOUND.into_response(),
        Ok(diff) => (StatusCode::OK, Json(DiffResponse { diff })).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

fn parse_filter(filter: Option<&str>) -> Option<TaskFilter> { match filter.unwrap_or("today") {
    "all" => Some(TaskFilter::All), "running" => Some(TaskFilter::Running), "today" => Some(TaskFilter::Today), _ => None,
} }

fn ensure_task_exists(store: &Store, id: &str) -> Result<(), StatusCode> {
    store.get_task(id).map_err(internal_error)?.ok_or(StatusCode::NOT_FOUND).map(|_| ())
}

fn read_task_output(task: &Task) -> String {
    if let Some(path) = task.output_path.as_deref()
        && let Ok(content) = std::fs::read_to_string(path)
    {
        return content;
    }
    if let Some(path) = task.log_path.as_deref()
        && let Ok(content) = std::fs::read_to_string(path)
    {
        let output = content
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter_map(|value| {
                value
                    .get("content")
                    .and_then(|content| content.as_str())
                    .map(str::to_string)
            })
            .collect::<String>();
        if !output.is_empty() {
            return output;
        }
    }
    "No output available".to_string()
}

fn internal_error(_: anyhow::Error) -> StatusCode { StatusCode::INTERNAL_SERVER_ERROR }

fn action_error(error: anyhow::Error) -> (StatusCode, Json<ActionResponse>) {
    let message = error.to_string();
    let status = if message.contains("not found") {
        StatusCode::NOT_FOUND
    } else if message.contains("not running") || message.contains("only DONE tasks") {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    (status, Json(ActionResponse { ok: false, new_task_id: None, error: Some(message) }))
}

fn diff_unavailable(diff: &str) -> bool { diff.contains("(worktree removed or diff unavailable)")
    || diff.contains("(no worktree diff or output file available)")
    || diff.contains("(in-place edit — no uncommitted changes detected, may already be committed)")
}
