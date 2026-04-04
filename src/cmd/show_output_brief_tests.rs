// Tests for full-default and brief output rendering.
// Exports: none; covers output_text and output_text_brief behavior.
// Deps: show_output hub, Store, Task, tempfile.
use super::*;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

fn test_task(id: &str, worktree: &Path) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: Some(worktree.display().to_string()),
        worktree_branch: None,
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
    }
}

fn numbered_assistant_log(count: usize, body_len: usize) -> String {
    (0..count)
        .map(|index| {
            serde_json::to_string(&json!({
                "type": "message",
                "role": "assistant",
                "content": format!("message-{index:02}-{}", "x".repeat(body_len)),
            }))
        })
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n")
}

#[test]
fn output_text_defaults_to_full() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(crate::paths::logs_dir()).unwrap();
    std::fs::write(crate::paths::log_path("t-output-full"), numbered_assistant_log(22, 500))
        .unwrap();

    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&test_task("t-output-full", temp.path())).unwrap();

    let output = output_text(&store, "t-output-full").unwrap();
    let parts = output.split("\n---\n").collect::<Vec<_>>();

    assert_eq!(parts.len(), 22);
    assert!(!output.contains("[... "));
    assert!(parts[0].starts_with("message-00-"));
    assert!(parts[21].starts_with("message-21-"));
}

#[test]
fn output_text_brief_truncates() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(crate::paths::logs_dir()).unwrap();
    std::fs::write(crate::paths::log_path("t-output-brief"), numbered_assistant_log(22, 500))
        .unwrap();

    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&test_task("t-output-brief", temp.path()))
        .unwrap();

    let output = output_text_brief(&store, "t-output-brief").unwrap();
    let parts = output.split("\n---\n").collect::<Vec<_>>();

    assert_eq!(parts.len(), 11);
    assert!(parts[3].starts_with("[... 12 messages omitted ...]"));
    assert!(parts.iter().all(|part| part.len() <= 1_000));
    assert!(output.len() <= 8_000);
}
