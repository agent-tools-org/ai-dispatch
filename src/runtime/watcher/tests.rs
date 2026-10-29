// Tests for watcher event parsing, synthetic milestones, and cost ceilings.
// Deps: super::*, types

use super::*;
use super::{SyntheticMilestoneTracker, parse_milestone_event};
use crate::agent::Agent;

use crate::types::{AgentKind, CompletionInfo, EventKind, Task, TaskEvent, TaskId, TaskStatus};

fn tracker_event(kind: EventKind, detail: &str) -> TaskEvent {
    TaskEvent::new(TaskId("t-synth".to_string()), kind, detail.to_string())
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
    let mut info = CompletionInfo::new(TaskStatus::Done);
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
        agent,
        verify_status: crate::types::VerifyStatus::Skipped,
        ..crate::types::Task::test_fixture(TaskId(id.to_string()), TaskStatus::Running)
    }
}

#[path = "milestone_tests.rs"]
mod milestone_tests;

#[path = "summary_tests.rs"]
mod summary_tests;

/// Tool-derived Error events carry the model's request, not the provider.
#[test]
fn tool_error_events_do_not_mark_quota() {
    let _temp = crate::support::test_env::TestHome::new();
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
    let _temp = crate::support::test_env::TestHome::new();
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
    let _temp = crate::support::test_env::TestHome::new();
    let store = std::sync::Arc::new(crate::store::Store::open_memory().unwrap());
    let task = Task {
        agent: AgentKind::Droid,
        verify_status: crate::types::VerifyStatus::Skipped,
        ..crate::types::Task::test_fixture(TaskId("t-droid-osc".to_string()), TaskStatus::Running)
    };
    store.insert_task(&task).unwrap();

    let mut synthetic = SyntheticMilestoneTracker::new();
    let mut info = CompletionInfo::new(TaskStatus::Done);
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
