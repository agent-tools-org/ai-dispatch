// Tests for OpenCode tool-call event parsing.
// Covers metadata.full preservation for long tool arguments.

use super::*;

#[test]
fn tool_call_args_over_cap_are_recoverable_from_metadata_full() {
    let args = format!("{{\"command\":\"{}\"}}", "cargo test --workspace ".repeat(8));
    let event = parse_json_event(
        AgentKind::OpenCode,
        AgentKind::OpenCode,
        None,
        &TaskId("t-tool".to_string()),
        &serde_json::json!({"type": "tool_call", "name": "bash", "arguments": args}),
        Local::now(),
    )
    .expect("tool_call events should parse");

    let expected_full = format!("bash: {args}");
    assert_eq!(event.event_kind, EventKind::Test);
    assert_eq!(event.detail.len(), super::super::truncate::EVENT_DETAIL_MAX);
    assert!(event.detail.ends_with("..."));
    assert_eq!(event.full_detail(), expected_full);
    assert_eq!(
        event.metadata.as_ref().and_then(|m| m["full"].as_str()),
        Some(expected_full.as_str())
    );
}

#[test]
fn tool_call_args_under_cap_keep_plain_detail() {
    let event = parse_json_event(
        AgentKind::OpenCode,
        AgentKind::OpenCode,
        None,
        &TaskId("t-tool".to_string()),
        &serde_json::json!({"type": "tool_call", "name": "read", "arguments": "src/main.rs"}),
        Local::now(),
    )
    .expect("tool_call events should parse");

    assert_eq!(event.detail, "read: src/main.rs");
    assert!(event.metadata.is_none());
}
