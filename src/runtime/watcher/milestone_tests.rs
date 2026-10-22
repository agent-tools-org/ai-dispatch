// milestone tests.
// Uses the shared test fixtures and imports from the parent test module.

use super::*;

#[test]
fn milestone_event_parses_plain_text_lines() {
    let event =
        parse_milestone_event(&TaskId("t-m1".to_string()), "[MILESTONE] types defined").unwrap();

    assert_eq!(event.event_kind, EventKind::Milestone);
    assert_eq!(event.detail, "types defined");
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

#[test]
fn synthetic_tracker_emits_milestone_after_three_reads() {
    let task_id = TaskId("t-read".to_string());
    let mut tracker = SyntheticMilestoneTracker::new();

    let mut milestone = None;
    for detail in ["Read", "Glob", "Read"] {
        let event = tracker_event(EventKind::ToolCall, detail);
        tracker.observe(&event);
        milestone = tracker.synthetic_event(&task_id, &event);
    }

    let milestone = milestone.expect("expected synthetic milestone");
    assert_eq!(milestone.event_kind, EventKind::Milestone);
    assert_eq!(milestone.detail, "[exploring] read 3 files");
}

#[test]
fn synthetic_tracker_emits_first_edit_after_reads() {
    let task_id = TaskId("t-edit".to_string());
    let mut tracker = SyntheticMilestoneTracker::new();

    for detail in ["Read", "Read", "Glob"] {
        let event = tracker_event(EventKind::ToolCall, detail);
        tracker.observe(&event);
        let _ = tracker.synthetic_event(&task_id, &event);
    }

    let edit = tracker_event(EventKind::ToolCall, "Edit");
    tracker.observe(&edit);
    let milestone = tracker
        .synthetic_event(&task_id, &edit)
        .expect("expected first edit");

    assert_eq!(milestone.detail, "[implementing] first edit");
}

#[test]
fn synthetic_tracker_stays_disabled_when_reasoning_exists() {
    let task_id = TaskId("t-reason".to_string());
    let mut tracker = SyntheticMilestoneTracker::new();
    let reasoning = tracker_event(EventKind::Reasoning, "thinking");
    tracker.observe(&reasoning);

    let tool = tracker_event(EventKind::ToolCall, "Read");
    tracker.observe(&tool);

    assert!(tracker.synthetic_event(&task_id, &tool).is_none());
}
