// Tests for MCP transport framing and tool dispatch.
// Deps: parent MCP handlers, in-memory Store, and Cursor.

use super::{TransportMode, handle_message, read_message};
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::io::Cursor;
use std::sync::Arc;

#[test]
fn reads_json_line_messages() {
    let mut reader = Cursor::new(br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#);
    let message = read_message(&mut reader).unwrap().unwrap();
    assert!(matches!(message.mode, TransportMode::JsonLine));
    assert!(message.body.contains(r#""method":"tools/list""#));
}

#[test]
fn reads_content_length_messages() {
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    let framed = format!("Content-Length: {}\r\n\r\n{body}", body.len());
    let mut reader = Cursor::new(framed.into_bytes());
    let message = read_message(&mut reader).unwrap().unwrap();
    assert!(matches!(message.mode, TransportMode::Framed));
    assert_eq!(message.body, body);
}

#[tokio::test]
async fn dispatches_board_tool_calls() {
    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&sample_task()).unwrap();

    let response = handle_message(
        store,
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"aid_board","arguments":{}}}"#,
    )
    .await
    .unwrap();
    let result = response.result.unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();

    assert!(text.contains("t-1234"));
    assert!(text.contains("codex"));
}

fn sample_task() -> Task {
    Task {
        id: TaskId("t-1234".to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "Investigate the failing MCP test".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        project_id: None,
        worktree_path: None,
        effective_dir: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: Some(42),
        prompt_tokens: None,
        duration_ms: Some(1_500),
        requested_model: Some("gpt-5".to_string()),
        observed_model: None,
        attribution_source: None,
        cost_usd: Some(0.01),
        exit_code: None,
        created_at: Local::now(),
        completed_at: Some(Local::now()),
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    }
}
