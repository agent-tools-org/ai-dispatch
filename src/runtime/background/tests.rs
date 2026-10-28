// Tests for background worker persistence and zombie-task cleanup.
// Covers spec serialization and store reconciliation side effects.

use super::*;
use chrono::{Duration, Local};

use super::{
    BackgroundRunSpec, ZOMBIE_FAILURE_DETAIL, build_on_done_command, check_zombie_tasks_with,
    cleanup_stale_pending_tasks, fail_stale_pending_task, kill, reaper, save_spec, waiting,
};
use crate::store::Store;
use crate::support::paths;
use crate::support::test_subprocess;
use crate::types::{AgentKind, EventKind, PendingReason, Task, TaskEvent, TaskId, TaskStatus};

fn make_spec(task_id: &str) -> BackgroundRunSpec {
    BackgroundRunSpec {
        task_id: task_id.to_string(),
        worker_pid: Some(if task_id == "t-1a1a" { 101 } else { 202 }),
        dir: Some(".".to_string()),
        budget: false,
        max_duration_mins: None,
        max_duration_secs: None,
        idle_timeout_secs: None,
        interactive: true,
        on_done: None,
        env: None,
        agent_pid: None,
        agent_started: None,
        audit_report_mode: false,
        pre_task_dirty_paths: None,
        foreground: false,
    }
}

fn make_task(task_id: &str, status: TaskStatus) -> Task {
    crate::types::Task::test_fixture(TaskId(task_id.to_string()), status)
}

fn insert_event(store: &Store, task_id: &str, age_secs: i64) {
    store
        .insert_event(&TaskEvent {
            task_id: TaskId(task_id.to_string()),
            timestamp: Local::now() - Duration::seconds(age_secs),
            event_kind: EventKind::Milestone,
            detail: "last progress".to_string(),
            metadata: None,
        })
        .unwrap();
}

fn insert_idle_escalation_event(store: &Store, task_id: &str, age_secs: i64) {
    store
        .insert_event(&TaskEvent {
            task_id: TaskId(task_id.to_string()),
            timestamp: Local::now() - Duration::seconds(age_secs),
            event_kind: EventKind::Milestone,
            detail: "Auto-escalated: task stalled".to_string(),
            metadata: Some(serde_json::json!({ "auto_escalated": true })),
        })
        .unwrap();
}

#[path = "spec_persistence_tests.rs"]
mod spec_persistence_tests;

#[path = "first_token_tests.rs"]
mod first_token_tests;

#[path = "pending_cleanup_tests.rs"]
mod pending_cleanup_tests;

#[path = "worker_cleanup_tests.rs"]
mod worker_cleanup_tests;

#[path = "reaper_policy_tests.rs"]
mod reaper_policy_tests;
