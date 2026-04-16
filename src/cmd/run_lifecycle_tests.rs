// Tests for post-run lifecycle dirty-state safeguards.
// Covers final dirty worktree assertion behavior around task status/events.
// Deps: run_lifecycle helpers, Store, git CLI, tempfile.

use super::final_dirty_assertion;
use crate::{store::Store, test_subprocess, types::*};
use chrono::Local;
use std::{path::Path, process::Command};

fn git(dir: &Path, args: &[&str]) {
    assert!(Command::new("git").arg("-C").arg(dir).args(args).status().unwrap().success());
}

fn init_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init"]);
    dir
}

fn write_path(dir: &Path, path: &str, content: &str) {
    let file = dir.join(path);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(file, content).unwrap();
}

fn task(id: &str, status: TaskStatus) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
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
        audit_verdict: None,
        audit_report_path: None,
    }
}

#[test]
fn final_assertion_fails_task_when_worktree_still_dirty() {
    let _permit = test_subprocess::acquire();
    let dir = init_repo();
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-dirty".to_string());
    store.insert_task(&task(task_id.as_str(), TaskStatus::Done)).unwrap();
    write_path(dir.path(), "src/lib.rs", "pub fn value() -> u8 { 1 }\n");

    let failed = final_dirty_assertion(&store, &task_id, dir.path().to_str().unwrap(), false).unwrap();

    assert!(failed);
    assert_eq!(store.get_task(task_id.as_str()).unwrap().unwrap().status, TaskStatus::Failed);
    let events = store.get_events(task_id.as_str()).unwrap();
    assert!(events.iter().any(|event| event.detail.contains("FAIL: agent left uncommitted changes after rescue and retry")));
    assert!(events.iter().any(|event| event.detail.contains("?? src/lib.rs")));
}

#[test]
fn final_assertion_skipped_for_read_only() {
    let _permit = test_subprocess::acquire();
    let dir = init_repo();
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-readonly".to_string());
    store.insert_task(&task(task_id.as_str(), TaskStatus::Done)).unwrap();
    write_path(dir.path(), "result-t-readonly.md", "audit output\n");

    let failed = final_dirty_assertion(&store, &task_id, dir.path().to_str().unwrap(), true).unwrap();

    assert!(!failed);
    assert_eq!(store.get_task(task_id.as_str()).unwrap().unwrap().status, TaskStatus::Done);
    let events = store.get_events(task_id.as_str()).unwrap();
    assert!(!events.iter().any(|event| event.detail.contains("FAIL: agent left uncommitted changes")));
}
