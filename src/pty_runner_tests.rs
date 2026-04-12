// Tests for PTY-backed background agent execution.
// Covers immediate task failure when the agent process cannot spawn.

use super::run_agent_process;
use crate::agent::kilo::KiloAgent;
use crate::paths;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::sync::Arc;

#[test]
fn run_agent_process_marks_task_failed_when_spawn_fails() {
    let aid_home = tempfile::tempdir().unwrap();
    let _guard = crate::paths::AidHomeGuard::set(aid_home.path());
    crate::paths::ensure_dirs().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-kilo-missing".to_string());
    store
        .insert_task(&task_fixture(&task_id, TaskStatus::Running))
        .unwrap();
    let log_path = paths::log_path(task_id.as_str());
    let cmd = std::process::Command::new("/definitely/missing/kilo");

    let err = run_agent_process(
        &KiloAgent,
        &cmd,
        &task_id,
        &store,
        &log_path,
        None,
        None,
        true,
    )
    .err()
    .unwrap();

    assert!(err.to_string().contains("Failed to spawn agent process"));
    assert_eq!(
        store.get_task(task_id.as_str()).unwrap().unwrap().status,
        TaskStatus::Failed
    );
    assert!(store
        .latest_error(task_id.as_str())
        .unwrap()
        .contains("Failed to spawn agent process"));
    assert!(std::fs::read_to_string(paths::stderr_path(task_id.as_str()))
        .unwrap()
        .contains("Failed to spawn agent process"));
}

fn task_fixture(id: &TaskId, status: TaskStatus) -> Task {
    Task {
        id: id.clone(),
        agent: AgentKind::Kilo,
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
