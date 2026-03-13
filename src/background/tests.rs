// Tests for background worker persistence and zombie-task cleanup.
// Covers spec serialization and store reconciliation side effects.

use chrono::Local;

use super::{BackgroundRunSpec, ZOMBIE_FAILURE_DETAIL, check_zombie_tasks_with, save_spec};
use crate::paths;
use crate::store::Store;
use crate::types::{AgentKind, EventKind, Task, TaskId, TaskStatus};

#[test]
fn serializes_spec_to_json() {
    let spec = BackgroundRunSpec {
        task_id: "t-save".to_string(),
        worker_pid: Some(4242),
        agent_name: "codex".to_string(),
        prompt: "prompt".to_string(),
        dir: Some(".".to_string()),
        output: None,
        model: None,
        verify: Some("auto".to_string()),
        retry: 2,
        group: Some("wg-demo".to_string()),
        skills: vec![],
    };

    let content = serde_json::to_string_pretty(&spec).unwrap();
    assert!(content.contains("\"agent_name\""));
    assert!(content.contains("\"codex\""));
}

#[test]
fn marks_running_background_tasks_failed_when_worker_is_missing() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().unwrap();

    let store = Store::open_memory().unwrap();
    store
        .insert_task(&make_task("t-live", TaskStatus::Running))
        .unwrap();
    store
        .insert_task(&make_task("t-zombie", TaskStatus::Running))
        .unwrap();
    store
        .insert_task(&make_task("t-foreground", TaskStatus::Running))
        .unwrap();
    save_spec(&make_spec("t-live")).unwrap();
    save_spec(&make_spec("t-zombie")).unwrap();

    let cleaned = check_zombie_tasks_with(&store, |pid| pid == 101).unwrap();

    assert_eq!(cleaned, vec!["t-zombie".to_string()]);
    assert_eq!(
        store.get_task("t-live").unwrap().unwrap().status,
        TaskStatus::Running
    );
    assert_eq!(
        store.get_task("t-zombie").unwrap().unwrap().status,
        TaskStatus::Failed
    );
    assert_eq!(
        store.get_task("t-foreground").unwrap().unwrap().status,
        TaskStatus::Running
    );

    let events = store.get_events("t-zombie").unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_kind, EventKind::Error);
    assert_eq!(events[0].detail, ZOMBIE_FAILURE_DETAIL);

    let stderr = std::fs::read_to_string(paths::stderr_path("t-zombie")).unwrap();
    assert_eq!(stderr.trim(), ZOMBIE_FAILURE_DETAIL);
}

fn make_spec(task_id: &str) -> BackgroundRunSpec {
    BackgroundRunSpec {
        task_id: task_id.to_string(),
        worker_pid: Some(if task_id == "t-live" { 101 } else { 202 }),
        agent_name: "codex".to_string(),
        prompt: "prompt".to_string(),
        dir: Some(".".to_string()),
        output: None,
        model: None,
        verify: None,
        retry: 0,
        group: None,
        skills: vec![],
    }
}

fn make_task(task_id: &str, status: TaskStatus) -> Task {
    Task {
        id: TaskId(task_id.to_string()),
        agent: AgentKind::Codex,
        prompt: "prompt".to_string(),
        status,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
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
    }
}
