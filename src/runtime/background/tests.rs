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
        interactive: true,
        ..Default::default()
    }
}

fn make_task(task_id: &str, status: TaskStatus) -> Task {
    crate::types::Task::test_fixture(TaskId(task_id.to_string()), status)
}

fn insert_event(store: &Store, task_id: &str, age_secs: i64) {
    store
        .insert_event(&TaskEvent::at(
            TaskId(task_id.to_string()),
            EventKind::Milestone,
            "last progress".to_string(),
            Local::now() - Duration::seconds(age_secs),
        ))
        .unwrap();
}

fn insert_idle_escalation_event(store: &Store, task_id: &str, age_secs: i64) {
    store
        .insert_event(&TaskEvent::at(
            TaskId(task_id.to_string()),
            EventKind::Milestone,
            "Auto-escalated: task stalled".to_string(),
            Local::now() - Duration::seconds(age_secs),
        ).with_metadata(Some(serde_json::json!({ "auto_escalated": true }))))
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
