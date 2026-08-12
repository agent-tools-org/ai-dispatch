// MCP report contract tests for task outcome and verification fields.
// Exports: module-scoped tests for aid MCP tools.
// Deps: mcp_tools, Store, serde_json, chrono.

use super::call_tool;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use serde_json::{Value, json};
use std::sync::Arc;

fn payload_text(result: &Value) -> &str {
    result["content"][0]["text"].as_str().expect("text content")
}

#[tokio::test]
async fn agents_tool_returns_fleet_payload() {
    let temp_dir = std::env::temp_dir().join("aid-mcp-agents-tool-test");
    let _home = crate::paths::AidHomeGuard::set(&temp_dir);
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
    let _home = crate::paths::AidHomeGuard::set(&temp_dir);
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
        id: TaskId("t-mcp-timeout".to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None, project_id: None,
        worktree_path: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None,
        observed_model: None,
        attribution_source: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: Some("cargo test".to_string()),
        verify_status: VerifyStatus::TimedOut,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
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
