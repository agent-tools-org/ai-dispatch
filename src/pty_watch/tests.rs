// Tests for PTY awaiting-input prompt capture.
// Covers metadata written for board-facing AWAIT reasons.
// Depends on pty_watch helpers and the in-memory Store.

use super::{extract_awaiting_prompt, mark_awaiting_input};
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus};
use chrono::Local;
use std::sync::Arc;

#[test]
fn stores_recent_question_as_awaiting_prompt_metadata() {
    let store = Arc::new(Store::open_memory().unwrap());
    let task = Task {
        id: TaskId("t-pty1".to_string()),
        agent: AgentKind::Codex,
        prompt: "prompt".to_string(),
        status: TaskStatus::Running,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
        log_path: None,
        output_path: None,
        tokens: None,
        duration_ms: None,
        model: None,
        cost_usd: None,
        created_at: Local::now(),
        completed_at: None,
    };
    store.insert_task(&task).unwrap();

    let prompt = "115:    use super::board::render_board;";
    let awaiting_prompt =
        extract_awaiting_prompt("Should I update board.rs?\n115:    use super::board::render_board;", prompt);
    let mut awaiting_input = false;
    mark_awaiting_input(&store, &task.id, prompt, &awaiting_prompt, &mut awaiting_input).unwrap();

    let event = store.get_events(task.id.as_str()).unwrap().pop().unwrap();
    assert_eq!(event.detail, prompt);
    assert_eq!(
        event.metadata.as_ref().and_then(|m| m.get("awaiting_prompt")).and_then(|v| v.as_str()),
        Some("Should I update board.rs?")
    );
}
