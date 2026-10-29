// summary tests.
// Uses the shared test fixtures and imports from the parent test module.

use super::*;
use serde_json::json;

#[test]
fn completion_metadata_updates_summary_fields() {
    let mut info = CompletionInfo::new(TaskStatus::Done);
    let event = TaskEvent::new(
        TaskId("t-usage".to_string()),
        EventKind::Completion,
        "completed".to_string(),
    ).with_metadata(Some(json!({
            "tokens": 12345,
            "model": "gpt-4.1",
            "cost_usd": 0.12
        })));

    apply_completion_event(&mut info, &event);

    assert_eq!(info.tokens, Some(12345));
    assert_eq!(info.model.as_deref(), Some("gpt-4.1"));
    assert_eq!(info.cost_usd, Some(0.12));
}

#[test]
fn non_completion_events_do_not_change_summary_fields() {
    let mut info = CompletionInfo {
        tokens: Some(10),
        status: TaskStatus::Done,
        model: Some("gpt-4.1".to_string()),
        cost_usd: Some(0.01),
        exit_code: None,
    };
    let event = TaskEvent::new(
        TaskId("t-ignore".to_string()),
        EventKind::Reasoning,
        "thinking".to_string(),
    ).with_metadata(Some(json!({ "tokens": 999 })));

    apply_completion_event(&mut info, &event);

    assert_eq!(info.tokens, Some(10));
    assert_eq!(info.model.as_deref(), Some("gpt-4.1"));
    assert_eq!(info.cost_usd, Some(0.01));
}

#[test]
fn cost_ceiling_only_triggers_above_limit() {
    assert!(!exceeds_cost_ceiling(Some(1.0), Some(1.0)));
    assert!(exceeds_cost_ceiling(Some(1.01), Some(1.0)));
    assert!(!exceeds_cost_ceiling(None, Some(1.0)));
    assert!(!exceeds_cost_ceiling(Some(1.0), None));
}



/// The live false positive of 2026-08-07 at the call site that produced it.
///
/// A cursor audit task quoted `src/agent/cursor_tests.rs:142` into its report.
/// Every chunk of that report reaches this function as `{"type":"assistant"}`,
/// which the adapter classifies as Reasoning — the model talking. Only a
/// diagnostic event is the CLI talking, and the model cannot author one.
#[test]
fn only_a_diagnostic_event_can_mark_a_route_rate_limited() {
    let _temp = crate::support::test_env::TestHome::new();
    let store = std::sync::Arc::new(crate::store::Store::open_memory().unwrap());
    let task = running_task("t-cursor-quote", AgentKind::Cursor);
    store.insert_task(&task).unwrap();
    crate::routing::rate_limit::clear_all_rate_limits_for_agent(&AgentKind::Cursor, None);

    let quoted = "assert_rate_limit(r#\"{\"type\":\"error\",\"message\":\"quota exceeded for \
                  this workspace\"}\"#, true);";
    let report = json!({
        "type": "assistant",
        "message": {"content": [{"type": "text", "text": quoted}]}
    })
    .to_string();

    feed_stream_line(&store, &task, &report);
    assert!(
        !crate::routing::rate_limit::is_rate_limited(&AgentKind::Cursor, None),
        "a report quoting our own fixture must not hold cursor"
    );

    let refusal = r#"{"type":"error","message":"quota exceeded for this workspace"}"#;
    feed_stream_line(&store, &task, refusal);
    assert!(
        crate::routing::rate_limit::is_rate_limited(&AgentKind::Cursor, None),
        "the same words from the CLI's own error envelope must still hold cursor"
    );
}
