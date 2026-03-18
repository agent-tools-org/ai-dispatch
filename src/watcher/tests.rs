// Tests for watcher event parsing — milestone/finding extraction, loop detection, cost ceiling.
// Deps: super::*, types

use super::{apply_completion_event, exceeds_cost_ceiling, parse_milestone_event, LoopDetector};
use crate::types::{CompletionInfo, EventKind, TaskEvent, TaskId, TaskStatus};
use chrono::Local;
use serde_json::json;

#[test]
fn completion_metadata_updates_summary_fields() {
    let mut info = CompletionInfo {
        tokens: None,
        status: TaskStatus::Done,
        model: None,
        cost_usd: None,
        exit_code: None,
    };
    let event = TaskEvent {
        task_id: TaskId("t-usage".to_string()),
        timestamp: Local::now(),
        event_kind: EventKind::Completion,
        detail: "completed".to_string(),
        metadata: Some(json!({
            "tokens": 12345,
            "model": "gpt-4.1",
            "cost_usd": 0.12
        })),
    };

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
    let event = TaskEvent {
        task_id: TaskId("t-ignore".to_string()),
        timestamp: Local::now(),
        event_kind: EventKind::Reasoning,
        detail: "thinking".to_string(),
        metadata: Some(json!({ "tokens": 999 })),
    };

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

#[test]
fn milestone_event_parses_plain_text_lines() {
    let event = parse_milestone_event(
        &TaskId("t-m1".to_string()),
        "[MILESTONE] types defined",
    )
    .unwrap();

    assert_eq!(event.event_kind, EventKind::Milestone);
    assert_eq!(event.detail, "types defined");
}

#[test]
fn milestone_event_parses_json_lines() {
    let line = r#"{"type":"item.completed","item":{"type":"agent_message","text":"[MILESTONE] tests passing\nnext"}} "#;
    let event = parse_milestone_event(&TaskId("t-m2".to_string()), line).unwrap();

    assert_eq!(event.event_kind, EventKind::Milestone);
    assert_eq!(event.detail, "tests passing");
}

#[test]
fn finding_event_parses_plain_text_lines() {
    let detail = super::extract_finding_detail("[FINDING] gamma can be zero in tricrypto");
    assert_eq!(detail.as_deref(), Some("gamma can be zero in tricrypto"));
}

#[test]
fn milestone_inside_string_literal_is_rejected() {
    let line = r#"println!("[MILESTONE] tests passing");"#;
    assert!(super::extract_milestone_detail(line).is_none());
}

#[test]
fn milestone_inside_json_string_value_is_rejected() {
    let line = r#"{"text": "assert_eq!(detail, "[MILESTONE] done")"}"#;
    assert!(super::extract_milestone_detail(line).is_none());
}

#[test]
fn finding_inside_string_literal_is_rejected() {
    let line = r#"let s = "[FINDING] gamma can be zero";"#;
    assert!(super::extract_finding_detail(line).is_none());
}

#[test]
fn real_milestone_still_extracted() {
    let detail = super::extract_milestone_detail("[MILESTONE] implementation complete");
    assert_eq!(detail.as_deref(), Some("implementation complete"));
}

#[test]
fn real_finding_still_extracted() {
    let detail = super::extract_finding_detail("[FINDING] pool has degenerate state");
    assert_eq!(detail.as_deref(), Some("pool has degenerate state"));
}

#[test]
fn milestone_lines_stripped_from_output() {
    let input = "line1\n[MILESTONE] types defined\nline2\n";
    let filtered: String = input
        .lines()
        .filter(|line| super::extract_milestone_detail(line).is_none())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(filtered, "line1\nline2");
}

fn loop_detector_case<I>(expected: bool, events: I) where I: IntoIterator<Item = &'static str> {
    let mut detector = LoopDetector::new();
    events.into_iter().for_each(|detail| detector.push(detail));
    assert_eq!(detector.is_looping(), expected);
}
#[test]
fn loop_detector_patterns() {
    loop_detector_case(false, ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
    loop_detector_case(true, std::iter::repeat("repeat").take(10));
    loop_detector_case(
        false,
        std::iter::repeat("dup").take(7).chain(["unique-1", "unique-2", "unique-3"]),
    );
    loop_detector_case(
        true,
        std::iter::repeat("dup").take(8).chain(["unique-1", "unique-2"]),
    );
}

#[test]
fn loop_detector_ignores_empty_details() {
    // Empty/whitespace details should not trigger loop detection
    loop_detector_case(false, std::iter::repeat("").take(20));
    loop_detector_case(false, std::iter::repeat("  ").take(20));
    loop_detector_case(false, std::iter::repeat("\t").take(20));
    // Mix of empty and real events should not false-positive
    let mut events: Vec<&str> = Vec::new();
    for _ in 0..5 {
        events.push("");
        events.push("working");
        events.push("  ");
    }
    loop_detector_case(false, events);
}
