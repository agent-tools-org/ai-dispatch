// Tests for PTY monitor log sanitization.
// Covers raw chunk writes so PTY logs match stdout escape stripping.
// Depends on MonitorState, Store, and tempfile-backed log files.

use super::MonitorState;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::sync::Arc;

#[test]
fn handle_chunk_strips_terminal_escapes_before_log_write() {
    let store = Arc::new(Store::open_memory().unwrap());
    let task = pty_task("t-pty-clean-log");
    store.insert_task(&task).unwrap();
    let mut state = MonitorState::with_policy(
        true,
        None,
        crate::config::timeout_policy::TimeoutPolicy::default(),
    );
    let mut log = tempfile::NamedTempFile::new().unwrap();

    state
        .handle_chunk(
            &crate::agent::codex::CodexAgent,
            &task.id,
            &store,
            log.as_file_mut(),
            "\x1b]0;title\x07\x1b[32mclean line\x1b[0m\n".to_string(),
        )
        .unwrap();

    let bytes = std::fs::read(log.path()).unwrap();
    assert!(!bytes.contains(&0x1b));
    assert_eq!(String::from_utf8(bytes).unwrap(), "clean line\n");
}

#[test]
fn output_after_pty_write_never_acknowledges_input() {
    use crate::types::{MessageDirection, MessageSource};
    let _permit = crate::support::test_subprocess::acquire();
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = pty_task("t-unconfirmed-input");
    task.agent = AgentKind::Cursor;
    store.insert_task(&task).unwrap();
    store.insert_message(task.id.as_str(), MessageDirection::In, "pivot",
        MessageSource::Reply).unwrap();
    let command = ["/bin/sh", "-c", "read line"].map(String::from).to_vec();
    let mut bridge = crate::runtime::pty::bridge::PtyBridge::spawn(&command, None, vec![]).unwrap();
    let mut state = MonitorState::with_policy(
        false,
        None,
        crate::config::timeout_policy::TimeoutPolicy::default(),
    );
    state.maybe_consume_reply(&mut bridge, &store, &task.id).unwrap();
    let mut log = tempfile::NamedTempFile::new().unwrap();
    state.handle_chunk(&crate::agent::cursor::CursorAgent, &task.id, &store,
        log.as_file_mut(), "pivot\nunrelated output\n".to_string()).unwrap();
    state.line_buffer.push_str("trailing output");
    state.flush_trailing_output(&crate::agent::cursor::CursorAgent, &task.id, &store).unwrap();
    assert!(bridge.wait().unwrap().success());
    let messages = store.list_messages_for_task(task.id.as_str()).unwrap();
    assert!(messages[0].delivered_at.is_some());
    assert!(serde_json::to_value(&messages[0]).unwrap().get("acked_at").is_none());
    let events = store.get_events(task.id.as_str()).unwrap();
    assert!(!events.iter().any(|event| event.detail.contains("Acked reply")));
    let metadata = events[0].metadata.as_ref().unwrap();
    assert_eq!(metadata["input_delivery"], "written");
    assert_eq!(metadata["consumption"], "unconfirmed");
    assert!(metadata.get("delivered").is_none());
}

fn pty_task(task_id: &str) -> Task {
    Task {
        id: TaskId(task_id.to_string()),
        agent: AgentKind::Codex,
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
