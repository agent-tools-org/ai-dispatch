// Tests for watcher event parsing, synthetic milestones, and cost ceilings.
// Deps: super::*, types

use super::{
    apply_completion_event, exceeds_cost_ceiling, parse_milestone_event, SyntheticMilestoneTracker,
};
use crate::paths;
use crate::types::{CompletionInfo, EventKind, TaskEvent, TaskId, TaskStatus, Task, AgentKind};
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

fn tracker_event(kind: EventKind, detail: &str) -> TaskEvent {
    TaskEvent {
        task_id: TaskId("t-synth".to_string()),
        timestamp: Local::now(),
        event_kind: kind,
        detail: detail.to_string(),
        metadata: None,
    }
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
    let milestone = tracker.synthetic_event(&task_id, &edit).expect("expected first edit");

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

#[test]
fn term_escape_strip_allows_droid_stream_parse_via_stream_path() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let store = std::sync::Arc::new(crate::store::Store::open_memory().unwrap());
    let task = Task {
        id: TaskId("t-droid-osc".to_string()),
        agent: AgentKind::Droid,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Running,
        parent_task_id: None,
        workgroup_id: None,
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
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None, observed_model: None, attribution_source: None,
        cost_usd: None,
        exit_code: None,
        created_at: chrono::Local::now(),
        completed_at: None,
        verify: None,
        verify_status: crate::types::VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    };
    store.insert_task(&task).unwrap();

    let mut synthetic = SyntheticMilestoneTracker::new();
    let mut info = CompletionInfo { tokens: None, status: TaskStatus::Done, model: None, cost_usd: None, exit_code: None };
    let mut event_count = 0u32;
    let mut session_saved = false;
    let agent = crate::agent::droid::DroidAgent;
    let line = "\x1b]9;4;0;\x07{\"type\":\"usage\",\"input_tokens\":1,\"output_tokens\":2,\"cost_usd\":0.0,\"model\":\"x\"}";

    let ctx = super::StreamLineContext {
        agent: &agent,
        task_id: &task.id,
        store: &store,
        workgroup_id: None,
        synthetic_tracker: &mut synthetic,
    };

    let res = super::handle_streaming_line_with_session(ctx, &mut info, &mut event_count, line, &mut session_saved).unwrap();
    assert!(res.is_some());
    assert_eq!(res.unwrap().kind, EventKind::Completion);
}

/// The live false positive of 2026-08-07 at the call site that produced it.
///
/// A cursor audit task quoted `src/agent/cursor_tests.rs:142` into its report.
/// Every chunk of that report reaches this function as `{"type":"assistant"}`,
/// which the adapter classifies as Reasoning — the model talking. Only a
/// diagnostic event is the CLI talking, and the model cannot author one.
#[test]
fn only_a_diagnostic_event_can_mark_a_route_rate_limited() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let store = std::sync::Arc::new(crate::store::Store::open_memory().unwrap());
    let task = running_task("t-cursor-quote", AgentKind::Cursor);
    store.insert_task(&task).unwrap();
    crate::rate_limit::clear_all_rate_limits_for_agent(&AgentKind::Cursor, None);

    let quoted = "assert_rate_limit(r#\"{\"type\":\"error\",\"message\":\"quota exceeded for \
                  this workspace\"}\"#, true);";
    let report = json!({
        "type": "assistant",
        "message": {"content": [{"type": "text", "text": quoted}]}
    })
    .to_string();

    feed_stream_line(&store, &task, &report);
    assert!(
        !crate::rate_limit::is_rate_limited(&AgentKind::Cursor, None),
        "a report quoting our own fixture must not hold cursor"
    );

    let refusal = r#"{"type":"error","message":"quota exceeded for this workspace"}"#;
    feed_stream_line(&store, &task, refusal);
    assert!(
        crate::rate_limit::is_rate_limited(&AgentKind::Cursor, None),
        "the same words from the CLI's own error envelope must still hold cursor"
    );
}

fn feed_stream_line(store: &std::sync::Arc<crate::store::Store>, task: &Task, line: &str) {
    let mut synthetic = SyntheticMilestoneTracker::new();
    let mut info = CompletionInfo { tokens: None, status: TaskStatus::Done, model: None, cost_usd: None, exit_code: None };
    let mut event_count = 0u32;
    let mut session_saved = false;
    let agent = crate::agent::cursor::CursorAgent;
    let ctx = super::StreamLineContext {
        agent: &agent,
        task_id: &task.id,
        store,
        workgroup_id: None,
        synthetic_tracker: &mut synthetic,
    };
    super::handle_streaming_line_with_session(ctx, &mut info, &mut event_count, line, &mut session_saved)
        .expect("stream line handled");
}

fn running_task(id: &str, agent: AgentKind) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Running,
        parent_task_id: None,
        workgroup_id: None,
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
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None,
        observed_model: None,
        attribution_source: None,
        cost_usd: None,
        exit_code: None,
        created_at: chrono::Local::now(),
        completed_at: None,
        verify: None,
        verify_status: crate::types::VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    }
}
