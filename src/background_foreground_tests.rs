// Foreground-spec coverage for background zombie reconciliation.
// Exercises foreground worker_pid specs through the existing orphan reaper.
// Deps: background cleanup internals, Store, paths, and task/event types.
use chrono::Local;

use super::{BackgroundRunSpec, check_zombie_tasks_with, save_spec};
use crate::paths;
use crate::store::Store;
use crate::types::{AgentKind, EventKind, Task, TaskEvent, TaskFilter, TaskId, TaskStatus, VerifyStatus};

fn make_task(task_id: &str) -> Task {
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

fn make_foreground_spec(task_id: &str, worker_pid: u32) -> BackgroundRunSpec {
    BackgroundRunSpec {
        task_id: task_id.to_string(),
        worker_pid: Some(worker_pid),
        agent_name: "codex".to_string(),
        prompt: "prompt".to_string(),
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
        idle_timeout_secs: Some(60),
        retry: 0,
        group: None,
        skills: vec![],
        checklist: vec![],
        template: None,
        interactive: true,
        on_done: None,
        cascade: vec![],
        parent_task_id: None,
        env: None,
        env_forward: None,
        agent_pid: None,
        sandbox: false,
        read_only: false,
        audit_report_mode: false,
        container: None,
        link_deps: true,
        pre_task_dirty_paths: None,
    }
}

fn insert_stale_event(store: &Store, task_id: &str) {
    store
        .insert_event(&TaskEvent {
            task_id: TaskId(task_id.to_string()),
            timestamp: Local::now() - chrono::Duration::seconds(61),
            event_kind: EventKind::Milestone,
            detail: "last output".to_string(),
            metadata: None,
        })
        .expect("insert event");
}

#[test]
fn foreground_spec_with_dead_worker_pid_is_reaped() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    let store = Store::open_memory().expect("store");
    let task = make_task("t-fg-dead");
    store.insert_task(&task).expect("insert task");
    save_spec(&make_foreground_spec("t-fg-dead", 987_654)).expect("save spec");
    insert_stale_event(&store, "t-fg-dead");

    let cleaned = check_zombie_tasks_with(&store, |_| false).expect("zombie cleanup");

    assert_eq!(cleaned, vec!["t-fg-dead".to_string()]);
    assert_eq!(
        store.get_task("t-fg-dead").expect("get task").expect("task").status,
        TaskStatus::Failed
    );
}

#[test]
fn foreground_spec_with_current_worker_pid_is_not_reaped() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    let store = Store::open_memory().expect("store");
    let task = make_task("t-fg-live");
    store.insert_task(&task).expect("insert task");
    let worker_pid = std::process::id();
    save_spec(&make_foreground_spec("t-fg-live", worker_pid)).expect("save spec");
    insert_stale_event(&store, "t-fg-live");

    let cleaned = check_zombie_tasks_with(&store, |pid| pid == worker_pid).expect("zombie cleanup");

    assert!(cleaned.is_empty());
    assert_eq!(
        store.get_task("t-fg-live").expect("get task").expect("task").status,
        TaskStatus::Running
    );
    assert_eq!(store.list_tasks(TaskFilter::Running).expect("running tasks").len(), 1);
}
