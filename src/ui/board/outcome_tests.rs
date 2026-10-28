// Board outcome rendering regressions: inconclusive verification, unknown cost totals.
// Exports: module-scoped tests.
// Deps: board::render_board, Store, and task outcome types.

use super::render_board;
use chrono::Local;



use crate::store::Store;
use crate::types::{Task, TaskId, TaskStatus, VerifyStatus};

fn timed_out_task() -> Task {
    Task {
        prompt: "test prompt".to_string(), project_id: crate::config::project::current_project_id(),
        verify: Some("cargo test".to_string()),
        verify_status: VerifyStatus::TimedOut,
        ..crate::types::Task::test_fixture(TaskId("t-timeout".to_string()), TaskStatus::Done)
    }
}

#[test]
fn board_marks_timed_out_verification_without_counting_success() {
    let _temp = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();

    let output = render_board(&[timed_out_task()], &store).unwrap();

    assert!(output.contains("[VTIMEOUT]"), "output: {output}");
    assert!(output.contains("1 total | 0 done"), "output: {output}");
}

#[test]
fn infrastructure_verification_renders_its_own_tag() {
    let _temp = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    let mut task = timed_out_task();
    task.verify_status = VerifyStatus::InfrastructureFailure;
    store.insert_task(&task).expect("task");
    store.insert_event(&crate::types::TaskEvent {
        task_id: task.id.clone(), timestamp: Local::now(),
        event_kind: crate::types::EventKind::Milestone,
        detail: "Verification could not run: rbox: Box lock not acquired within 60 s".into(),
        metadata: Some(serde_json::json!({"verify_execution_error": true})),
    }).expect("event");

    let output = render_board(&[task], &store).unwrap();
    assert!(output.contains("rbox: Box lock not acquired within 60 s"), "{output}");

    assert!(output.contains("[VINFRA]"), "output: {output}");
    assert!(!output.contains("[VFAIL]"), "output: {output}");
}

#[test]
fn board_omits_verification_tags_when_no_verification_result_exists() {
    let _temp = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    let mut failed = timed_out_task();
    failed.id = TaskId("t-failed".to_string());
    failed.status = TaskStatus::Failed;
    failed.verify = None;
    failed.verify_status = VerifyStatus::Skipped;
    let mut running = timed_out_task();
    running.id = TaskId("t-running".to_string());
    running.status = TaskStatus::Running;
    running.verify = None;
    running.verify_status = VerifyStatus::Skipped;

    let output = render_board(&[failed, running], &store).unwrap();

    assert!(!output.contains("[VSKIPPED]"), "output: {output}");
    assert!(!output.contains("[V"), "output: {output}");
}

#[test]
fn board_counts_only_active_lifecycle_statuses_as_running() {
    let _temp = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    let mut waiting = timed_out_task();
    waiting.id = TaskId("t-waiting".to_string());
    waiting.status = TaskStatus::Waiting;
    waiting.verify = None;
    waiting.verify_status = VerifyStatus::Skipped;
    let mut pending = waiting.clone();
    pending.id = TaskId("t-pending".to_string());
    pending.status = TaskStatus::Pending;
    let mut running = waiting.clone();
    running.id = TaskId("t-running".to_string());
    running.status = TaskStatus::Running;

    let output = render_board(&[pending, running, waiting], &store).unwrap();

    assert!(output.contains("3 total | 0 done | 1 running | 0 failed"), "output: {output}");
}

#[test]
fn board_marks_unobserved_detach_without_counting_success() {
    let _temp = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    let mut task = timed_out_task();
    task.id = TaskId("t-unobserved".to_string());
    task.verify = None;
    task.verify_status = VerifyStatus::Unobserved;

    let output = render_board(&[task], &store).unwrap();

    assert!(output.contains("[VNORESULT]"), "output: {output}");
    assert!(output.contains("1 total | 0 done"), "output: {output}");
}

#[test]
fn board_does_not_count_stopped_tasks_as_failed() {
    let _temp = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    let mut stopped = timed_out_task();
    stopped.status = TaskStatus::Stopped;
    stopped.verify_status = VerifyStatus::Skipped;
    stopped.verify = None;

    let output = render_board(&[stopped], &store).unwrap();

    assert!(output.contains("1 total | 0 done | 0 running | 0 failed"), "output: {output}");
}

#[test]
fn board_total_names_unknown_costs_instead_of_hiding_them() {
    let _temp = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    let mut unknown = timed_out_task();
    unknown.tokens = Some(5_000);
    let mut priced = timed_out_task();
    priced.id = TaskId("t-priced".to_string());
    priced.tokens = Some(1_000);
    priced.cost_usd = Some(0.25);
    let all_unknown = render_board(std::slice::from_ref(&unknown), &store).unwrap();
    assert!(all_unknown.contains("Cost: unknown (1 tasks)"), "{all_unknown}");
    let partial = render_board(&[unknown, priced], &store).unwrap();
    assert!(partial.contains("Cost: $0.25 + 1 unknown"), "{partial}");
}
