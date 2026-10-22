// Tests for watcher event parsing, synthetic milestones, and cost ceilings.
// Deps: super::*, types

use super::*;
use super::{
    SyntheticMilestoneTracker, apply_completion_event, exceeds_cost_ceiling, parse_milestone_event,
};
use crate::agent::Agent;
use crate::support::paths;
use crate::types::{AgentKind, CompletionInfo, EventKind, Task, TaskEvent, TaskId, TaskStatus};
use chrono::Local;
use serde_json::json;

fn tracker_event(kind: EventKind, detail: &str) -> TaskEvent {
    TaskEvent {
        task_id: TaskId("t-synth".to_string()),
        timestamp: Local::now(),
        event_kind: kind,
        detail: detail.to_string(),
        metadata: None,
    }
}

fn feed_stream_line(store: &std::sync::Arc<crate::store::Store>, task: &Task, line: &str) {
    feed_stream_line_with_agent(store, task, line, &crate::agent::cursor::CursorAgent);
}

fn feed_stream_line_with_agent(
    store: &std::sync::Arc<crate::store::Store>,
    task: &Task,
    line: &str,
    agent: &dyn Agent,
) {
    let mut synthetic = SyntheticMilestoneTracker::new();
    let mut info = CompletionInfo {
        tokens: None,
        status: TaskStatus::Done,
        model: None,
        cost_usd: None,
        exit_code: None,
    };
    let mut event_count = 0u32;
    let mut session_saved = false;
    let ctx = super::StreamLineContext {
        agent,
        task_id: &task.id,
        store,
        workgroup_id: None,
        synthetic_tracker: &mut synthetic,
    };
    super::handle_streaming_line_with_session(
        ctx,
        &mut info,
        &mut event_count,
        line,
        &mut session_saved,
    )
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

#[path = "milestone_tests.rs"]
mod milestone_tests;

#[path = "summary_tests.rs"]
mod summary_tests;

#[test]
fn tool_error_events_do_not_mark_quota() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());
    let store = std::sync::Arc::new(crate::store::Store::open_memory().unwrap());
    let task = running_task("t-copilot-tool", AgentKind::Copilot);
    store.insert_task(&task).unwrap();
    crate::routing::rate_limit::clear_all_rate_limits_for_agent(&AgentKind::Copilot, None);
    let line = r#"{"type":"tool.execution_complete","data":{"success":false,"toolName":"grep","error":"You have exceeded your monthly quota"}}"#;
    feed_stream_line_with_agent(&store, &task, line, &crate::agent::copilot::CopilotAgent);
    assert!(
        !crate::routing::rate_limit::is_rate_limited(&AgentKind::Copilot, None),
        "a failing tool must not write a copilot hold"
    );
}

#[test]
fn streamed_opencode_refusal_holds_only_the_dispatched_provider() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let store = std::sync::Arc::new(crate::store::Store::open_memory().unwrap());
    let mut task = running_task("t-opencode-provider-scope", AgentKind::OpenCode);
    task.requested_model = Some("opencode/deepseek-v4-pro".to_string());
    store.insert_task(&task).unwrap();
    crate::routing::rate_limit::clear_all_rate_limits_for_agent(&AgentKind::OpenCode, None);

    let refusal = r#"{"type":"error","timestamp":1776000000000,"sessionID":"ses_live","error":{"name":"APIError","data":{"message":"Insufficient balance. Manage your billing here: https://opencode.ai/workspace/wrk_live/billing","statusCode":401,"isRetryable":false,"responseHeaders":{},"responseBody":"{\"type\":\"error\",\"error\":{\"type\":\"CreditsError\"}}","metadata":{"url":"https://opencode.ai/zen/v1/chat/completions"}}}}"#;
    feed_stream_line_with_agent(
        &store,
        &task,
        refusal,
        crate::agent::get_agent(AgentKind::OpenCode).as_ref(),
    );

    assert!(!crate::routing::rate_limit::is_rate_limited(
        &AgentKind::OpenCode,
        None
    ));
    assert!(
        crate::routing::rate_limit::dispatch_blocking_hold_for_model(
            &AgentKind::OpenCode,
            None,
            Some("opencode/deepseek-v4-pro"),
        )
        .is_some()
    );
    assert!(
        crate::routing::rate_limit::dispatch_blocking_hold_for_model(
            &AgentKind::OpenCode,
            None,
            Some("opencode-go/deepseek-v4-pro"),
        )
        .is_none()
    );
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
    };
    store.insert_task(&task).unwrap();

    let mut synthetic = SyntheticMilestoneTracker::new();
    let mut info = CompletionInfo {
        tokens: None,
        status: TaskStatus::Done,
        model: None,
        cost_usd: None,
        exit_code: None,
    };
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

    let res = super::handle_streaming_line_with_session(
        ctx,
        &mut info,
        &mut event_count,
        line,
        &mut session_saved,
    )
    .unwrap();
    assert!(res.is_some());
    assert_eq!(res.unwrap().kind, EventKind::Completion);
}

#[test]
fn milestone_event_parses_json_lines() {
    let line = r#"{"type":"item.completed","item":{"type":"agent_message","text":"[MILESTONE] tests passing\nnext"}} "#;
    let event = parse_milestone_event(&TaskId("t-m2".to_string()), line).unwrap();

    assert_eq!(event.event_kind, EventKind::Milestone);
    assert_eq!(event.detail, "tests passing");
}
