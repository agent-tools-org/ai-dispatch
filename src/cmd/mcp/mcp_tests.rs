// Tests for MCP transport framing and tool dispatch.
// Deps: parent MCP handlers, in-memory Store, and Cursor.

use super::{TransportMode, handle_message, read_message};
use crate::store::Store;
use crate::types::{Task, TaskId, TaskStatus};
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
        prompt: "Investigate the failing MCP test".to_string(),
        tokens: Some(42),
        duration_ms: Some(1_500),
        requested_model: Some("gpt-5".to_string()),
        cost_usd: Some(0.01),
        created_at: Local::now(),
        completed_at: Some(Local::now()),
        ..crate::types::Task::test_fixture(TaskId("t-1234".to_string()), TaskStatus::Done)
    }
}
