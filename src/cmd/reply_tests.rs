// Tests for tracked reply queuing and acknowledgement polling.
// Deps: reply handlers, Store, temporary AID homes, and task types.

use std::time::Duration;

use chrono::Local;

use super::{InputCommand, ReplyOutcome, run, run_with_hook, run_with_hook_and_sleep};
use crate::paths::AidHomeGuard;
use crate::store::Store;
use crate::types::{AgentKind, MessageSource, Task, TaskId, TaskStatus, VerifyStatus};

fn make_task(id: &str, status: TaskStatus) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "test".to_string(),
        resolved_prompt: None,
        category: None,
        status,
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
        created_at: Local::now(),
        completed_at: None,
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

#[test]
fn reply_async_returns_immediately() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = AidHomeGuard::set(temp.path());
    let store = Store::open_memory().unwrap();
    store.insert_task(&make_task("t-reply-async", TaskStatus::Running)).unwrap();

    let outcome = run(&store, "t-reply-async", Some("follow this path"), None, true, 30).unwrap();

    assert_eq!(outcome, ReplyOutcome::Queued { id: 1 });
    let messages = store.list_messages_for_task("t-reply-async").unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].source, MessageSource::Reply);
    assert_eq!(messages[0].content, "follow this path");
}

#[test]
fn reply_polls_until_ack() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = AidHomeGuard::set(temp.path());
    let store = Store::open_memory().unwrap();
    store.insert_task(&make_task("t-reply-ack", TaskStatus::AwaitingInput)).unwrap();

    let mut polls = 0usize;
    let outcome = run_with_hook_and_sleep(
        &store,
        "t-reply-ack",
        Some("answer"),
        None,
        false,
        Duration::from_millis(25),
        Duration::from_millis(1),
        MessageSource::Reply,
        InputCommand::Reply,
        |message_id| {
            polls += 1;
            if polls == 1 {
                store.mark_delivered(message_id).unwrap();
            }
            if polls == 2 {
                store.mark_acked_latest_inbound("t-reply-ack").unwrap();
            }
        },
        |_| {},
    )
    .unwrap();

    assert_eq!(outcome, ReplyOutcome::Acked { delivered: true });
}

#[test]
fn reply_times_out_cleanly() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = AidHomeGuard::set(temp.path());
    let store = Store::open_memory().unwrap();
    store.insert_task(&make_task("t-reply-timeout", TaskStatus::Stalled)).unwrap();

    let outcome = run_with_hook(
        &store,
        "t-reply-timeout",
        Some("nudge"),
        None,
        false,
        Duration::from_millis(5),
        Duration::from_millis(1),
        MessageSource::Reply,
        InputCommand::Reply,
        |_| {},
    )
    .unwrap();

    assert_eq!(outcome, ReplyOutcome::TimedOut { delivered: false });
}
