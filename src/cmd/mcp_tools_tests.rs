// MCP report contract tests for task outcome and verification fields.
// Exports: module-scoped tests for aid MCP tools.
// Deps: mcp_tools, Store, serde_json, chrono.

use super::call_tool;
use crate::store::Store;
use crate::types::{Task, TaskId, TaskStatus, VerifyStatus};

use serde_json::{Value, json};
use std::sync::Arc;

fn payload_text(result: &Value) -> &str {
    result["content"][0]["text"].as_str().expect("text content")
}

#[tokio::test]
async fn agents_tool_returns_fleet_payload() {
    let temp_dir = std::env::temp_dir().join("aid-mcp-agents-tool-test");
    let _home = crate::support::paths::AidHomeGuard::set(&temp_dir);
    let store = Arc::new(Store::open_memory().expect("memory store"));

    let result = call_tool(store, "aid_agents", json!({}))
        .await
        .expect("call aid_agents");

    assert!(result.get("isError").is_none(), "{result}");
    let payload: Value = serde_json::from_str(payload_text(&result)).expect("payload JSON");
    assert!(payload["generated_at"].is_string());
    let agents = payload["agents"].as_array().expect("agents array");
    assert!(!agents.is_empty());
    assert!(agents.iter().all(|agent| agent["name"].is_string()));
    assert!(agents.iter().all(|agent| agent["quota"]["state"].is_string()));
}

#[tokio::test]
async fn advise_tool_returns_declared_advice_payload() {
    let temp_dir = std::env::temp_dir().join("aid-mcp-advise-tool-test");
    let _home = crate::support::paths::AidHomeGuard::set(&temp_dir);
    let store = Arc::new(Store::open_memory().expect("memory store"));

    let result = call_tool(
        store,
        "aid_advise",
        json!({
            "prompt": "Refactor src/main.rs into smaller modules",
            "difficulty": "complex",
            "budget": "premium",
            "urgency": "urgent",
            "rigor": "standard",
            "top": 3
        }),
    )
    .await
    .expect("call aid_advise");

    assert!(result.get("isError").is_none(), "{result}");
    let payload: Value = serde_json::from_str(payload_text(&result)).expect("payload JSON");
    assert_eq!(payload["declared"]["difficulty"], "complex");
    assert_eq!(payload["declared"]["budget"], "premium");
    assert_eq!(payload["declared"]["urgency"], "urgent");
    assert_eq!(payload["declared"]["rigor"], "standard");
    let candidates = payload["candidates"].as_array().expect("candidates array");
    assert!(!candidates.is_empty());
    assert!(candidates.len() <= 3);
}

#[tokio::test]
async fn advise_tool_errors_on_unknown_dimension_value() {
    let store = Arc::new(Store::open_memory().expect("memory store"));

    let result = call_tool(
        store,
        "aid_advise",
        json!({
            "prompt": "Refactor src/main.rs",
            "difficulty": "impossible",
            "budget": "premium",
            "urgency": "urgent",
            "rigor": "critical"
        }),
    )
    .await
    .expect("call aid_advise");

    assert_eq!(result["isError"], true);
    assert!(payload_text(&result).contains("Unknown difficulty 'impossible'"));
}

#[tokio::test]
async fn advise_tool_requires_declared_dimensions() {
    let store = Arc::new(Store::open_memory().expect("memory store"));

    let result = call_tool(
        store,
        "aid_advise",
        json!({ "prompt": "Refactor src/main.rs", "difficulty": "complex" }),
    )
    .await
    .expect("call aid_advise");

    assert_eq!(result["isError"], true);
    assert!(payload_text(&result).contains("Invalid arguments for aid_advise"));
}

#[test]
fn run_tool_report_adds_outcome_and_verification_status() {
    let task_id = TaskId("t-mcp-run".to_string());

    let payload = super::run_task_report(&task_id, None);

    assert_eq!(payload["task_id"], "t-mcp-run");
    assert_eq!(payload["status"], "pending");
    assert_eq!(payload["outcome"], "in_progress");
    assert_eq!(payload["verify_status"], "pending");
}

#[tokio::test]
async fn board_tool_reports_outcome_and_verification_status() {
    let store = Arc::new(Store::open_memory().expect("memory store"));
    let task = Task {
        verify: Some("cargo test".to_string()),
        verify_status: VerifyStatus::TimedOut,
        ..crate::types::Task::test_fixture(TaskId("t-mcp-timeout".to_string()), TaskStatus::Done)
    };
    store.insert_task(&task).expect("insert task");

    let result = call_tool(store, "aid_board", json!({}))
        .await
        .expect("call aid_board");
    let payload: Value = serde_json::from_str(payload_text(&result)).expect("payload JSON");

    assert_eq!(payload["tasks"][0]["status"], "done");
    assert_eq!(payload["tasks"][0]["outcome"], "unverified");
    assert_eq!(payload["tasks"][0]["verify_status"], "timed_out");
}

#[test]
fn board_group_sql_preserves_mcp_rows_on_many_tied_tasks() {
    let (_home, store) = crate::support::test_env::TestHome::new().shared_store();
    for i in 0..150 {
        store
            .db()
            .execute(
                "INSERT INTO tasks (id, agent, prompt, resolved_prompt, status, created_at,
             duration_ms, workgroup_id)
             VALUES (?1, 'cursor', ?2, ?2, 'done', '2026-10-01T00:00:00Z', 1000, ?3)",
                rusqlite::params![
                    format!("t-{i}"),
                    "prompt".repeat(1000),
                    if i % 3 == 0 { "selected" } else { "other" }
                ],
            )
            .unwrap();
    }
    let expected = store
        .list_tasks(crate::types::TaskFilter::All)
        .unwrap()
        .into_iter()
        .filter(|task| task.workgroup_id.as_deref() == Some("selected"))
        .map(super::render_board_task)
        .collect::<Vec<_>>();
    let actual = super::board_tool(store, json!({"group": "selected"})).unwrap();
    assert_eq!(actual, json!({"tasks": expected}));
}
