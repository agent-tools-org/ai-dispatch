// Terminal persistence regressions for every failed/stopped Store writer.
// Checks timing, no late terminal events, and stop refusal attribution.
// Deps: Store mutations, task lifecycle and stop command fixtures.

use super::{stop, tests::make_task};
use crate::store::{Store, TaskCompletionUpdate};
use crate::types::{EventKind, PendingReason, TaskEvent, TaskId, TaskStatus};
use chrono::{Duration, Local};
use std::sync::Arc;

fn payload(id: &str, status: TaskStatus) -> TaskCompletionUpdate<'_> {
    TaskCompletionUpdate {
        exit_code: Some(1),
        ..TaskCompletionUpdate::new(id, status, 1234)
    }
}

fn event(id: &str) -> TaskEvent {
    TaskEvent::new(TaskId(id.into()), EventKind::Error, "worker failure".into())
}

#[test]
fn settle_first_every_failed_stopped_writer_records_terminal_timing() {
    let store = Store::open_memory().expect("store");
    for (index, status) in [
        TaskStatus::Running,
        TaskStatus::Running,
        TaskStatus::Running,
        TaskStatus::Pending,
        TaskStatus::Waiting,
        TaskStatus::Running,
        TaskStatus::Running,
        TaskStatus::Running,
        TaskStatus::Done,
    ]
    .into_iter()
    .enumerate()
    {
        assert_writer_timing(&store, index, status);
    }
}

fn assert_writer_timing(store: &Store, index: usize, status: TaskStatus) {
    let id = format!("t-settle-timing-{index}");
    let mut task = make_task(&id, status);
    task.created_at = Local::now() - Duration::seconds(60);
    store.insert_task(&task).expect("insert");
    if index == 0 {
        store
            .db()
            .execute(
                "UPDATE tasks SET started_at = ?2 WHERE id = ?1",
                rusqlite::params![id, (Local::now() - Duration::seconds(3)).to_rfc3339()],
            )
            .expect("started_at");
    }
    let changed = match index {
        0 => store.update_task_status(&id, TaskStatus::Failed),
        1 => store.update_task_status(&id, TaskStatus::Stopped),
        2 => store.fail_if_running(&id),
        3 => store.settle_task_status(&id, TaskStatus::Failed, Some(&[TaskStatus::Pending]),
            Some(PendingReason::Unknown), None,
        ),
        4 => store.fail_waiting_with_reason(&id, "timeout"),
        5 => store.fail_active_execution(&id),
        6 => store.record_agent_completion(payload(&id, TaskStatus::Failed), None),
        7 => store.record_agent_completion(payload(&id, TaskStatus::Stopped), Some(&event(&id))),
        _ => store.fail_completed_verify_gate(&id),
    }
    .expect("write");
    assert!(changed, "writer {index}");
    let task = store.get_task(&id).expect("get").expect("task");
    assert!(task.completed_at.is_some(), "writer {index} completed_at");
    let duration = task.duration_ms.expect("terminal duration");
    if index == 0 {
        assert!(
            (2900..10000).contains(&duration),
            "duration from started_at: {duration}"
        );
    } else if matches!(index, 6 | 7) {
        assert_eq!(duration, 1234, "worker-measured duration");
    } else {
        assert!(duration >= 59000, "writer {index}: duration {duration}");
    }
}

#[test]
fn settle_first_worker_failure_after_stop_leaves_row_and_events_unchanged() {
    let _home = crate::support::test_env::TestHome::new();
    let store = Arc::new(Store::open_memory().expect("store"));
    store
        .insert_task(&make_task("t-stop-worker", TaskStatus::Running))
        .expect("insert");
    stop(&store, "t-stop-worker").expect("stop");
    let before = store.get_task("t-stop-worker").expect("get").expect("task");
    assert!(!crate::task::lifecycle::mark_failed(&store, &before.id).expect("late failure"));
    assert!(
        !store
            .record_agent_completion(
                payload("t-stop-worker", TaskStatus::Failed),
                Some(&event("t-stop-worker"))
            )
            .expect("late completion")
    );
    assert!(!store.fail_active_execution("t-stop-worker").expect("late reaper"));
    let after = store.get_task("t-stop-worker").expect("get").expect("task");
    assert_eq!(after.status, TaskStatus::Stopped);
    assert_eq!(after.completed_at, before.completed_at);
    assert_eq!(after.duration_ms, before.duration_ms);
    assert_eq!(store.get_events("t-stop-worker").expect("events").len(), 1);
}

#[test]
fn settle_first_terminal_writers_do_not_replace_any_terminal_row() {
    let store = Store::open_memory().expect("store");
    for status in [
        TaskStatus::Done,
        TaskStatus::Failed,
        TaskStatus::Stopped,
        TaskStatus::Skipped,
        TaskStatus::Merged,
    ] {
        let id = format!("t-settle-{}", status.as_str());
        let mut task = make_task(&id, status);
        task.completed_at = Some(Local::now() - Duration::minutes(1));
        task.duration_ms = Some(100);
        store.insert_task(&task).expect("insert");
        assert!(
            !store
                .update_task_status(&id, TaskStatus::Failed)
                .expect("failure")
        );
        assert!(!store.update_task_status(&id, TaskStatus::Stopped).expect("stop"));
        assert!(!store.fail_completed_verify_gate(&id).expect("verify"));
        assert!(
            !store
                .record_agent_completion(payload(&id, TaskStatus::Failed), Some(&event(&id)))
                .expect("completion")
        );
        let after = store.get_task(&id).expect("get").expect("task");
        assert_eq!(after.status, status);
        assert_eq!(after.completed_at, task.completed_at);
        assert_eq!(after.duration_ms, Some(100));
        assert!(store.get_events(&id).expect("events").is_empty());
    }
}

#[test]
fn settle_first_stop_preserves_winning_terminal_reason_without_event() {
    let _home = crate::support::test_env::TestHome::new();
    let store = Arc::new(Store::open_memory().expect("store"));
    store
        .insert_task(&make_task("t-stop-lost", TaskStatus::Running))
        .expect("insert");
    crate::support::paths::ensure_dirs().expect("dirs");
    crate::runtime::background::record_failure(&store, "t-stop-lost", "worker died", "worker died")
        .expect("reaper wins");
    let before = store.get_events("t-stop-lost").expect("events");
    stop(&store, "t-stop-lost").expect("terminal stop settles worker");
    assert_eq!(
        store
            .get_task("t-stop-lost")
            .expect("task")
            .expect("row")
            .status,
        TaskStatus::Failed
    );
    assert!(
        store
            .get_events("t-stop-lost")
            .expect("events")
            .iter().any(|event| event.detail == "worker died")
    );
    assert_eq!(
        store.get_events("t-stop-lost").expect("events").len(),
        before.len()
    );
}
