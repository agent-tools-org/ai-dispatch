// Tests for web API handlers and JSON response serialization.
// Exports: none.
// Deps: tokio, tempfile, crate::web::api, crate::store, crate::types.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Local;
use tempfile::NamedTempFile;

use super::api::{
    ActionResponse, DiffResponse, TaskEventResponse, TaskListParams, TaskResponse, get_task, get_task_events,
    get_task_output, get_usage, list_tasks,
};
use crate::store::Store;
use crate::types::{AgentKind, EventKind, Task, TaskEvent, TaskId, TaskStatus, VerifyStatus};

fn make_task(id: &str) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: Some("resolved".to_string()),
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: Some("wg-1".to_string()),
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
        log_path: None,
        output_path: None,
        tokens: Some(42),
        prompt_tokens: Some(11),
        duration_ms: Some(5000),
        model: Some("gpt".to_string()),
        cost_usd: Some(0.5),
        exit_code: Some(0),
        created_at: Local::now(),
        completed_at: Some(Local::now()),
        verify: Some("cargo check".to_string()),
        verify_status: VerifyStatus::Passed,
        read_only: false,
        budget: false,
    }
}

#[test]
fn task_response_serializes_rfc3339_timestamps() {
    let json = serde_json::to_value(TaskResponse::from_task(make_task("t-1"), None, None)).unwrap();
    assert!(json["created_at"].as_str().unwrap().contains('T'));
    assert!(json["completed_at"].as_str().unwrap().contains('T'));
    assert_eq!(json["status"], "done");
}

#[test]
fn task_event_response_serializes_timestamp() {
    let event = TaskEvent {
        task_id: TaskId("t-1".to_string()),
        timestamp: Local::now(),
        event_kind: EventKind::Milestone,
        detail: "done".to_string(),
        metadata: None,
    };
    let json = serde_json::to_value(TaskEventResponse::from(event)).unwrap();
    assert!(json["timestamp"].as_str().unwrap().contains('T'));
    assert_eq!(json["event_kind"], "milestone");
}

#[test]
fn action_response_serializes_ok() {
    let json = serde_json::to_value(ActionResponse {
        ok: true,
        new_task_id: Some("t-2".to_string()),
        error: None,
    })
    .unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["new_task_id"], "t-2");
    assert!(json.get("error").is_none());
}

#[test]
fn diff_response_serializes() {
    let json = serde_json::to_value(DiffResponse {
        diff: "diff --git a b".to_string(),
    })
    .unwrap();
    assert_eq!(json["diff"], "diff --git a b");
}

#[tokio::test]
async fn list_tasks_returns_task_json() {
    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&make_task("t-1")).unwrap();
    let Json(tasks) = list_tasks(Query(TaskListParams { filter: Some("all".to_string()) }), State(store))
        .await
        .unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "t-1");
}

#[tokio::test]
async fn get_task_returns_404_for_missing_task() {
    let store = Arc::new(Store::open_memory().unwrap());
    let result = get_task(Path("missing".to_string()), State(store)).await;
    assert_eq!(result.unwrap_err(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_task_events_returns_serialized_events() {
    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&make_task("t-1")).unwrap();
    store
        .insert_event(&TaskEvent {
            task_id: TaskId("t-1".to_string()),
            timestamp: Local::now(),
            event_kind: EventKind::Milestone,
            detail: "built".to_string(),
            metadata: Some(serde_json::json!({"step": 1})),
        })
        .unwrap();
    let Json(events) = get_task_events(Path("t-1".to_string()), State(store)).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].detail, "built");
}

#[tokio::test]
async fn get_task_output_reads_output_file_before_log_file() {
    let store = Arc::new(Store::open_memory().unwrap());
    let output = NamedTempFile::new().unwrap();
    std::fs::write(output.path(), "final output").unwrap();
    let mut task = make_task("t-1");
    task.output_path = Some(output.path().display().to_string());
    store.insert_task(&task).unwrap();
    let Json(response) = get_task_output(Path("t-1".to_string()), State(store)).await.unwrap();
    assert_eq!(response.output, "final output");
}

#[tokio::test]
async fn get_usage_combines_success_rates_and_avg_costs() {
    let store = Arc::new(Store::open_memory().unwrap());
    for id in 0..5 {
        let mut task = make_task(&format!("t-{id}"));
        task.cost_usd = Some(1.0 + id as f64);
        store.insert_task(&task).unwrap();
    }
    let Json(response) = get_usage(State(store)).await.unwrap();
    assert_eq!(response.agents.len(), 1);
    assert_eq!(response.agents[0].agent, "codex");
    assert_eq!(response.agents[0].task_count, 5);
    assert!(response.agents[0].avg_cost.is_some());
}
