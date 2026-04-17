// Tests for auto-GC enablement and completion cleanup triggers.
// Covers project-default opt-in and settled-group cleanup behavior.
// Deps: super, crate::store::Store, crate::types, chrono.

use super::group_is_settled;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::sync::Arc;

fn task(id: &str, group: &str, status: TaskStatus) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status,
        parent_task_id: None,
        workgroup_id: Some(group.to_string()),
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
        delivery_assessment: None,
    }
}

#[test]
fn group_is_settled_requires_all_tasks_terminal() {
    let store = Arc::new(Store::open_memory().unwrap());
    for item in [
        task("t-1", "wg-1", TaskStatus::Done),
        task("t-2", "wg-1", TaskStatus::Failed),
        task("t-3", "wg-1", TaskStatus::Running),
    ] {
        store.insert_task(&item).unwrap();
    }

    assert!(!group_is_settled(&store, "wg-1").unwrap());
}

#[test]
fn group_is_settled_accepts_done_and_failed_tasks() {
    let store = Arc::new(Store::open_memory().unwrap());
    for item in [
        task("t-1", "wg-1", TaskStatus::Done),
        task("t-2", "wg-1", TaskStatus::Failed),
    ] {
        store.insert_task(&item).unwrap();
    }

    assert!(group_is_settled(&store, "wg-1").unwrap());
}
