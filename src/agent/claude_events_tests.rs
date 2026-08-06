// Tests for Claude stream-json event parsing.
// Covers rate-limit marking and detail capping with metadata.full.
// Loaded by `claude_events.rs` under `#[cfg(test)]`.
use super::*;
use crate::{paths, rate_limit};

#[test]
fn marks_claude_rate_limits_from_error_and_user_events() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    rate_limit::clear_rate_limit(&AgentKind::Claude);
    let task_id = TaskId("t-claude-rate".to_string());
    let event = parse_event_line(
        &task_id,
        r#"{"type":"error","message":"rate limit exceeded"}"#,
    )
    .unwrap();
    assert_eq!(event.event_kind, EventKind::Error);
    assert!(rate_limit::is_rate_limited(&AgentKind::Claude));
    rate_limit::clear_rate_limit(&AgentKind::Claude);
    let event = parse_event_line(
        &task_id,
        r#"{"type":"user","message":{"content":[{"content":"HTTP 429 too many requests","is_error":true}]}}"#,
    )
    .unwrap();
    assert_eq!(event.event_kind, EventKind::Error);
    assert!(rate_limit::is_rate_limited(&AgentKind::Claude));
    rate_limit::clear_rate_limit(&AgentKind::Claude);
}

#[test]
fn caps_long_assistant_text_and_keeps_full_in_metadata() {
    let task_id = TaskId("t-claude-cap".to_string());
    let long_text = format!("Reasoning {}", "w".repeat(120));
    let line = serde_json::json!({
        "type": "assistant",
        "message": { "content": [{ "type": "text", "text": long_text }] }
    })
    .to_string();
    let event = parse_event_line(&task_id, &line).unwrap();
    assert!(event.detail.len() <= crate::agent::truncate::EVENT_DETAIL_MAX);
    assert!(event.detail.ends_with("..."));
    let metadata = event.metadata.expect("metadata with full text");
    assert_eq!(metadata["full"].as_str(), Some(long_text.as_str()));
}

#[test]
fn short_assistant_text_has_no_full_metadata() {
    let task_id = TaskId("t-claude-short".to_string());
    let line = serde_json::json!({
        "type": "assistant",
        "message": { "content": [{ "type": "text", "text": "brief note" }] }
    })
    .to_string();
    let event = parse_event_line(&task_id, &line).unwrap();
    assert_eq!(event.detail, "brief note");
    assert!(event.metadata.is_none());
}
