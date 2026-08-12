// Owner-chain re-entry tests for nested worktree leases.
// Covers descendant allow and unrelated refuse on the same lock.
// Deps: worktree lock helpers, Store, tempfile.

use super::{
    check_worktree_lock, try_acquire_worktree_lock_with_store,
};
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use tempfile::TempDir;

fn make_task(id: &str, status: TaskStatus, parent: Option<&str>) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "test".to_string(),
        resolved_prompt: None,
        category: None,
        status,
        parent_task_id: parent.map(str::to_string),
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

#[test]
fn descendant_reenters_ancestor_held_worktree_unrelated_is_refused() {
    let store = Store::open_memory().expect("store should open");
    store
        .insert_task(&make_task("t-ancestor", TaskStatus::Running, None))
        .expect("ancestor");
    store
        .insert_task(&make_task("t-child", TaskStatus::Pending, Some("t-ancestor")))
        .expect("child");
    store
        .insert_task(&make_task("t-unrelated", TaskStatus::Pending, None))
        .expect("unrelated");

    let dir = TempDir::new().expect("tempdir should be created");
    try_acquire_worktree_lock_with_store(dir.path(), "t-ancestor", Some(&store))
        .expect("ancestor acquires");

    try_acquire_worktree_lock_with_store(dir.path(), "t-child", Some(&store))
        .expect("descendant must re-enter ancestor lease");
    assert_eq!(
        check_worktree_lock(dir.path()).as_deref(),
        Some("t-ancestor"),
        "re-entry must leave the ancestor lease in place"
    );

    let err = try_acquire_worktree_lock_with_store(dir.path(), "t-unrelated", Some(&store))
        .expect_err("unrelated task must still be refused");
    assert_eq!(err, "t-ancestor");
    println!(
        "PROOF re-entry: descendant t-child entered worktree held by t-ancestor; lock holder remains t-ancestor"
    );
    println!("PROOF refuse: unrelated t-unrelated refused with holder={err}");
}
