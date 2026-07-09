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
    let mut state = MonitorState::new(true, None);
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
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        model: None,
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
