// Unit tests for stop, kill, and retry-tree termination behavior.
// Covers status transitions, process cleanup, and worktree preservation.
// Deps: stop command internals, Store, background run specs.

use super::*;
use crate::paths::AidHomeGuard;
use crate::store::Store;
use crate::types::{AgentKind, EventKind, TaskId, VerifyStatus};
use chrono::Local;
use std::sync::Arc;
use tempfile::TempDir;

fn make_task(id: &str, status: TaskStatus) -> Task {
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
        repo_path: None, project_id: None,
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

fn with_store<T>(f: impl FnOnce(Arc<Store>) -> T) -> T {
    let temp = TempDir::new().unwrap();
    let _guard = AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    f(store)
}
#[test]
fn stop_missing_task_returns_error() {
    with_store(|store| {
        let err = stop(&store, "t-missing").unwrap_err();
        assert!(err.to_string().contains("Task 't-missing' not found"));
    });
}

#[test]
fn stop_done_task_returns_error() {
    with_store(|store| {
        let task = make_task("t-done", TaskStatus::Done);
        store.insert_task(&task).unwrap();
        let err = stop(&store, "t-done").unwrap_err();
        assert!(err.to_string().contains("already terminal"));
        let reloaded = store.get_task("t-done").unwrap().unwrap();
        assert_eq!(reloaded.status, TaskStatus::Done);
    });
}

fn assert_termination_sets_stopped(
    action: fn(&Arc<Store>, &str) -> Result<()>,
    task_id: &str,
    status: TaskStatus,
    detail: &str,
) {
    with_store(|store| {
        store.insert_task(&make_task(task_id, status)).unwrap();
        action(&store, task_id).unwrap();
        let updated = store.get_task(task_id).unwrap().unwrap();
        assert_eq!(updated.status, TaskStatus::Stopped);
        let events = store.get_events(task_id).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].detail, detail);
        assert_eq!(events[0].event_kind, EventKind::Completion);
    });
}

#[test]
fn stop_running_task_sets_stopped() {
    assert_termination_sets_stopped(stop, "t-aa01", TaskStatus::Running, "Task stopped by user");
}

#[test]
fn stop_waiting_task_sets_stopped() {
    assert_termination_sets_stopped(stop, "t-wait", TaskStatus::Waiting, "Task stopped by user");
}

#[test]
fn stop_pending_task_sets_stopped() {
    assert_termination_sets_stopped(stop, "t-pend", TaskStatus::Pending, "Task stopped by user");
}

#[test]
fn kill_waiting_task_sets_stopped() {
    assert_termination_sets_stopped(kill, "t-kill", TaskStatus::Waiting, "Task killed by user");
}

#[test]
fn stop_attempts_agent_cleanup_when_agent_pid_exists() {
    use crate::background::{save_spec, BackgroundRunSpec};
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();

    let store = Arc::new(Store::open_memory().unwrap());
    let task = make_task("t-3010", TaskStatus::Running);
    store.insert_task(&task).unwrap();

    let spec = BackgroundRunSpec {
        task_id: "t-3010".to_string(),
        worker_pid: Some(999999),
        agent_pid: Some(888888),
        agent_name: "codex".to_string(),
        prompt: "test".to_string(),
        dir: None,
        output: None,
        result_file: None,
        model: None,
        verify: None,
        setup: None,
        iterate: None,
        eval: None,
        eval_feedback_template: None,
        judge: None,
        max_duration_mins: None,
        idle_timeout_secs: None,
        retry: 0,
        group: None,
        skills: vec![],
        checklist: vec![],
        hooks: vec![],
        template: None,
        worktree: None,
        base_branch: None,
        peer_review: None,
        audit: false,
        scope: vec![],
        interactive: false,
        on_done: None,
        cascade: vec![],
        parent_task_id: None,
        env: None,
        env_forward: None,
        sandbox: false,
        read_only: false,
        audit_report_mode: false,
        container: None,
        link_deps: true,
        pre_task_dirty_paths: None,
    };
    save_spec(&spec).unwrap();

    let result = stop(&store, "t-3010");

    assert!(result.is_ok(), "stop should succeed even with non-existent PIDs");
    assert_eq!(
        store.get_task("t-3010").unwrap().unwrap().status,
        TaskStatus::Stopped
    );
}

#[test]
fn preserve_worktree_skips_read_only_tasks() {
    let mut task = make_task("t-ro01", TaskStatus::Running);
    task.read_only = true;
    preserve_worktree("t-ro01", &task, "stopped");
}

#[test]
fn preserve_worktree_attempts_commit_for_non_read_only() {
    use tempfile::TempDir;

    let temp = TempDir::new().unwrap();
    let temp_path = temp.path().to_str().unwrap().to_string();

    let mut task = make_task("t-write01", TaskStatus::Running);
    task.worktree_path = Some(temp_path.clone());

    preserve_worktree("t-write01", &task, "stopped");
}

// Issue #112 — `aid stop --retry-tree`.
fn make_child(id: &str, parent: &str, status: TaskStatus) -> Task {
    let mut t = make_task(id, status);
    t.parent_task_id = Some(parent.to_string());
    t
}

fn insert_chain(store: &Arc<Store>, tasks: &[Task]) {
    for t in tasks {
        store.insert_task(t).unwrap();
    }
}

#[test]
fn stop_retry_tree_stops_root_and_running_descendants() {
    with_store(|store| {
        insert_chain(
            &store,
            &[
                make_task("t-root", TaskStatus::Running),
                make_child("t-a", "t-root", TaskStatus::Done),
                make_child("t-b", "t-root", TaskStatus::Running),
                make_child("t-c", "t-b", TaskStatus::Pending),
            ],
        );

        stop_retry_tree(&store, "t-root", false).unwrap();

        // Root + the two non-terminal descendants are now Stopped.
        assert_eq!(
            store.get_task("t-root").unwrap().unwrap().status,
            TaskStatus::Stopped
        );
        assert_eq!(
            store.get_task("t-b").unwrap().unwrap().status,
            TaskStatus::Stopped
        );
        assert_eq!(
            store.get_task("t-c").unwrap().unwrap().status,
            TaskStatus::Stopped
        );
        // The already-terminal one is unchanged.
        assert_eq!(
            store.get_task("t-a").unwrap().unwrap().status,
            TaskStatus::Done
        );
    });
}

#[test]
fn stop_retry_tree_resolves_to_root_from_descendant() {
    with_store(|store| {
        insert_chain(
            &store,
            &[
                make_task("t-root2", TaskStatus::Running),
                make_child("t-mid", "t-root2", TaskStatus::Running),
                make_child("t-leaf", "t-mid", TaskStatus::Running),
            ],
        );

        // Pass a descendant — should still stop the entire tree.
        stop_retry_tree(&store, "t-leaf", false).unwrap();

        for id in ["t-root2", "t-mid", "t-leaf"] {
            assert_eq!(
                store.get_task(id).unwrap().unwrap().status,
                TaskStatus::Stopped,
                "expected {id} stopped"
            );
        }
    });
}

#[test]
fn stop_retry_tree_missing_task_errors() {
    with_store(|store| {
        let err = stop_retry_tree(&store, "t-nope", false).unwrap_err();
        assert!(err.to_string().contains("not found"));
    });
}
