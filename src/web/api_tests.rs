// Tests for web API handlers and JSON response serialization.
// Exports: none.
// Deps: tokio, tempfile, crate::web::api, crate::store, crate::types.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Local;
use tempfile::NamedTempFile;

use super::api::{
    DiffResponse, TaskEventResponse, TaskListParams, TaskResponse, get_task, get_task_events,
    get_task_output, get_task_result, get_usage, list_tasks, merge_task, steer_task,
};
use super::api_types::{ActionResponse, MessageRequest, TaskEnrichment};
use super::fleet::{FleetParams, ServerInfo, get_fleet};
use crate::store::Store;
use crate::types::{
    AgentKind, DeliveryAssessment, EventKind, Task, TaskEvent, TaskId, TaskStatus, VerifyStatus,
};

pub(super) fn make_task(id: &str) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: Some("resolved".to_string()),
        category: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: Some("wg-1".to_string()),
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None, project_id: None,
        worktree_path: None, effective_dir: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: Some(42),
        prompt_tokens: Some(11),
        duration_ms: Some(5000),
        requested_model: Some("gpt".to_string()),
        observed_model: None, attribution_source: None,
        cost_usd: Some(0.5),
        exit_code: Some(0),
        created_at: Local::now(),
        completed_at: Some(Local::now()),
        verify: Some("cargo check".to_string()),
        verify_status: VerifyStatus::Passed,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    }
}

#[test]
fn task_response_serializes_rfc3339_timestamps() {
    let json = serde_json::to_value(TaskResponse::from_task(make_task("t-1"), TaskEnrichment::default())).unwrap();
    assert!(json["created_at"].as_str().unwrap().contains('T'));
    assert!(json["completed_at"].as_str().unwrap().contains('T'));
    assert_eq!(json["status"], "done");
    assert_eq!(json["outcome"], "verified");
}

#[test]
fn task_response_does_not_report_unobserved_as_success() {
    let mut task = make_task("t-unobs");
    task.verify = None;
    task.verify_status = VerifyStatus::Unobserved;
    let json = serde_json::to_value(TaskResponse::from_task(task, TaskEnrichment::default())).unwrap();
    assert_eq!(json["status"], "done");
    assert_eq!(json["verify_status"], "unobserved");
    assert_eq!(json["outcome"], "unverified");
    assert_ne!(json["outcome"], "delivered");
}

#[test]
fn task_response_serializes_delivery_assessment() {
    let mut task = make_task("t-1");
    task.pending_reason = Some("rate_limited".to_string());
    task.delivery_assessment = Some(DeliveryAssessment::EmptyDiff);

    let json = serde_json::to_value(TaskResponse::from_task(task, TaskEnrichment::default())).unwrap();

    assert_eq!(json["pending_reason"], "rate_limited");
    assert_eq!(json["delivery_assessment"], "empty_diff");
}

#[test]
fn task_response_keeps_unmeasured_values_null() {
    let mut task = make_task("t-unknown");
    task.cost_usd = None;
    task.requested_model = None;
    let value = serde_json::to_value(TaskResponse::from_task(task, TaskEnrichment::default())).unwrap();
    assert!(value["cost_usd"].is_null());
    assert!(value["observed_model"].is_null());
    assert!(value["difficulty"].is_null());
    assert!(value["rigor"].is_null());
    assert!(value["budget_class"].is_null());
    assert!(value["urgency"].is_null());
    assert!(value["memory_mb"].is_null());
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
async fn task_response_reports_started_at_after_running_transition() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = make_task("t-started");
    task.status = TaskStatus::Pending;
    store.insert_task(&task).unwrap();
    assert!(store.update_task_status("t-started", TaskStatus::Running).unwrap());
    let Json(response) = get_task(Path("t-started".to_string()), State(store)).await.unwrap();
    assert!(response.started_at.is_some());
}

#[tokio::test]
async fn fleet_returns_redacted_tasks_and_summary_in_one_snapshot() {
    let home = tempfile::tempdir().unwrap();
    let _home = crate::paths::AidHomeGuard::set(home.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = make_task("t-fleet");
    task.project_id = Some("client-api".to_string());
    task.cost_usd = None;
    store.insert_task(&task).unwrap();
    let Json(response) = get_fleet(
        Query(FleetParams { window: Some("all".to_string()) }),
        State(store),
        axum::Extension(ServerInfo {
            host: "127.0.0.1".to_string(),
            port: 8080,
            started_at: "2026-08-20T07:00:00Z".to_string(),
        }),
    )
    .await
    .unwrap();
    assert_eq!(response.summary.done, 1);
    assert_eq!(response.summary.spend_usd, None);
    assert_eq!(response.summary.tokens, Some(42));
    assert_eq!(response.sectors.len(), 1);
    assert!(!response.agents.is_empty());
    assert!(response.sectors[0].tasks[0].get("prompt").is_none());
    assert!(response.sectors[0].tasks[0].get("resolved_prompt").is_none());
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
async fn get_task_result_reads_persisted_report() {
    let home = tempfile::tempdir().unwrap();
    let _home = crate::paths::AidHomeGuard::set(home.path());
    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&make_task("t-result")).unwrap();
    let result_path = crate::paths::task_dir("t-result").join("result.md");
    std::fs::create_dir_all(result_path.parent().unwrap()).unwrap();
    std::fs::write(&result_path, "report").unwrap();
    let Json(response) = get_task_result(Path("t-result".to_string()), State(store)).await.unwrap();
    assert_eq!(response.result, "report");
}

#[tokio::test]
async fn steer_endpoint_uses_cli_guard_for_terminal_tasks() {
    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&make_task("t-steer-done")).unwrap();
    let expected = crate::cmd::steer::run(store.as_ref(), "t-steer-done", "pivot")
        .expect_err("CLI guard must reject terminal task")
        .to_string();
    let response = steer_task(
        Path("t-steer-done".to_string()),
        State(store),
        Json(MessageRequest { message: "pivot".to_string() }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(expected.contains("can only reply to running tasks"));
}

#[tokio::test]
async fn merge_endpoint_reports_running_task_as_conflict() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = make_task("t-merge-running");
    task.status = TaskStatus::Running;
    store.insert_task(&task).unwrap();
    let response = merge_task(Path(task.id.to_string()), State(store)).await.into_response();
    assert_eq!(response.status(), StatusCode::CONFLICT);
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
