// Unit tests for orphaned idle-task reaping and activity bookkeeping filters.
// Covers dead-worker hung detection and ignoring aid self-nudge/ack events.
// Deps: background_orphan helpers, store, and background specs.

use super::*;
use crate::background::{save_spec, BackgroundRunSpec};
use crate::paths;
use crate::store::Store;
use crate::types::{
    AgentKind, EventKind, Task, TaskEvent, TaskStatus, VerifyStatus,
};

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

fn make_spec(task_id: &str, worker_pid: Option<u32>, idle_timeout_secs: Option<u64>) -> BackgroundRunSpec {
    BackgroundRunSpec {
        task_id: task_id.to_string(),
        worker_pid,
        agent_name: "codex".to_string(),
        prompt: "prompt".to_string(),
        dir: Some(".".to_string()),
        output: None, result_file: None, model: None,
        verify: None, setup: None, iterate: None,
        eval: None,
        eval_feedback_template: None,
        judge: None,
        max_duration_mins: None,
        idle_timeout_secs,
        retry: 0,
        group: None,
        skills: vec![],
        checklist: vec![], hooks: vec![], template: None,
        worktree: None, base_branch: None, peer_review: None,
        audit: false, scope: vec![],
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

fn insert_event(store: &Store, task_id: &str, age_secs: i64, event_kind: EventKind) {
    store
        .insert_event(&TaskEvent {
            task_id: TaskId(task_id.to_string()),
            timestamp: Local::now() - chrono::Duration::seconds(age_secs),
            event_kind,
            detail: "last progress".to_string(),
            metadata: None,
        })
        .expect("insert event");
}

#[test]
fn orphan_reaper_fails_stale_task_when_worker_is_dead() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    let store = Store::open_memory().expect("store");
    let task = make_task("t-orph1");
    store.insert_task(&task).expect("insert task");
    save_spec(&make_spec("t-orph1", Some(77), Some(120))).expect("save spec");
    insert_event(&store, "t-orph1", 121, EventKind::Milestone);

    let cleaned = cleanup_orphaned_idle_tasks(&store, &[task], &[], &|_| false).expect("cleanup");

    assert_eq!(cleaned, vec!["t-orph1".to_string()]);
    assert_eq!(
        store.get_task("t-orph1").expect("get task").expect("task").status,
        TaskStatus::Failed
    );
    let events = store.get_events("t-orph1").expect("events");
    assert!(events.iter().any(|event| event.detail.contains("orphaned supervisor")));
    assert!(events.iter().any(|event| event.detail == "hung_detected"));
}

#[test]
fn orphan_reaper_keeps_stale_task_when_worker_is_alive() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    let store = Store::open_memory().expect("store");
    let task = make_task("t-live1");
    store.insert_task(&task).expect("insert task");
    save_spec(&make_spec("t-live1", Some(77), Some(120))).expect("save spec");
    insert_event(&store, "t-live1", 1_000, EventKind::Milestone);

    let cleaned = cleanup_orphaned_idle_tasks(&store, &[task], &[], &|pid| pid == 77).expect("cleanup");

    assert!(cleaned.is_empty());
    assert_eq!(
        store.get_task("t-live1").expect("get task").expect("task").status,
        TaskStatus::Running
    );
}

#[test]
fn orphan_reaper_treats_reasoning_as_activity_before_idle_timeout() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    let store = Store::open_memory().expect("store");
    let task = make_task("t-idle1");
    store.insert_task(&task).expect("insert task");
    save_spec(&make_spec("t-idle1", Some(77), Some(600))).expect("save spec");
    insert_event(&store, "t-idle1", 500, EventKind::Reasoning);

    let cleaned = cleanup_orphaned_idle_tasks(&store, &[task], &[], &|_| false).expect("cleanup");

    assert!(cleaned.is_empty());
    assert_eq!(
        store.get_task("t-idle1").expect("get task").expect("task").status,
        TaskStatus::Running
    );
}

#[test]
fn orphan_reaper_skips_tasks_without_background_spec() {
    let store = Store::open_memory().expect("store");
    let task = make_task("t-nospec");
    store.insert_task(&task).expect("insert task");
    insert_event(&store, "t-nospec", 1_000, EventKind::Milestone);

    let cleaned = cleanup_orphaned_idle_tasks(&store, &[task], &[], &|_| false).expect("cleanup");

    assert!(cleaned.is_empty());
    assert_eq!(
        store.get_task("t-nospec").expect("get task").expect("task").status,
        TaskStatus::Running
    );
}

#[test]
fn is_stale_requires_idle_timeout_to_elapse() {
    let now = Local::now();

    assert!(is_stale(now - chrono::Duration::seconds(300), now, 300));
    assert!(!is_stale(now - chrono::Duration::seconds(299), now, 300));
}

#[test]
fn latest_activity_ignores_setup_as_agent_output() {
    let store = Store::open_memory().expect("store");
    let mut task = make_task("t-setup-act");
    task.created_at = Local::now() - chrono::Duration::seconds(200);
    store.insert_task(&task).expect("insert task");
    store
        .insert_event(&TaskEvent {
            task_id: TaskId("t-setup-act".to_string()),
            timestamp: Local::now() - chrono::Duration::seconds(190),
            event_kind: EventKind::Setup,
            detail: "Cargo target seeded: /tmp/target from /cache in 12ms".to_string(),
            metadata: None,
        })
        .expect("insert setup");

    let activity = latest_activity(&store, &task).expect("activity");

    assert_eq!(activity.event_count, 0);
    assert_eq!(activity.timestamp, task.created_at);
    assert!(activity.detail.is_none());
}
