// Regression tests for Store status, completion and metadata mutations.
// Deps: in-memory Store, task/event types and completion payloads.

use super::mutations::*;
use crate::types::*;
use chrono::Local;
use rusqlite::params;
use crate::store::Store;

fn insert_task_status(store: &Store, task_id: &str, status: TaskStatus) {
    let conn = store.db();
    conn.execute(
        "INSERT INTO tasks (id, agent, prompt, status, created_at)
         VALUES (?1, 'codex', 'test prompt', ?2, '2026-03-18T00:00:00Z')",
        params![task_id, status.as_str()],
    )
    .unwrap();
}

#[test]
fn complete_task_atomic_writes_both_task_and_event() {
    let store = Store::open_memory().unwrap();
    insert_task_status(&store, "t-atomic", TaskStatus::Running);

    let event = TaskEvent {
        task_id: TaskId("t-atomic".to_string()),
        timestamp: Local::now(),
        event_kind: EventKind::Completion,
        detail: "completed atomically".to_string(),
        metadata: None,
    };
    store
        .complete_task_atomic(
            TaskCompletionUpdate {
                id: "t-atomic",
                status: TaskStatus::Done,
                tokens: Some(1234),
                duration_ms: 5000,
                observed_model: Some("test-model"),
                attribution_source: None,
                cost_usd: Some(0.05),
                exit_code: Some(0),
            },
            &event,
        )
        .unwrap();

    let task = store.get_task("t-atomic").unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Done);
    assert_eq!(task.tokens, Some(1234));
    assert_eq!(task.duration_ms, Some(5000));

    let events = store.get_events("t-atomic").unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_kind, EventKind::Completion);
    assert_eq!(events[0].detail, "completed atomically");
}

#[test]
fn complete_task_atomic_does_not_override_failed_status() {
    let store = Store::open_memory().unwrap();
    insert_task_status(&store, "t-atomic-failed", TaskStatus::Failed);

    let event = TaskEvent {
        task_id: TaskId("t-atomic-failed".to_string()),
        timestamp: Local::now(),
        event_kind: EventKind::Completion,
        detail: "completed after interrupt".to_string(),
        metadata: None,
    };
    store
        .complete_task_atomic(
            TaskCompletionUpdate {
                id: "t-atomic-failed",
                status: TaskStatus::Done,
                tokens: Some(1234),
                duration_ms: 5000,
                observed_model: Some("test-model"),
                attribution_source: None,
                cost_usd: Some(0.05),
                exit_code: Some(0),
            },
            &event,
        )
        .unwrap();

    let task = store.get_task("t-atomic-failed").unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(task.tokens, None);
    assert_eq!(task.completed_at, None);
}

#[test]
fn update_task_status_rejects_illegal_failed_to_done() {
    let store = Store::open_memory().unwrap();
    insert_task_status(&store, "t-guard-failed", TaskStatus::Failed);

    let changed = store
        .update_task_status("t-guard-failed", TaskStatus::Done)
        .unwrap();

    let task = store.get_task("t-guard-failed").unwrap().unwrap();
    assert!(!changed);
    assert_eq!(task.status, TaskStatus::Failed);
}

#[test]
fn rescue_task_to_done_allows_named_failed_to_done_exception() {
    let store = Store::open_memory().unwrap();
    insert_task_status(&store, "t-rescue", TaskStatus::Failed);

    let changed = store.rescue_task_to_done("t-rescue").unwrap();

    let task = store.get_task("t-rescue").unwrap().unwrap();
    assert!(changed);
    assert_eq!(task.status, TaskStatus::Done);
}

#[test]
fn complete_task_atomic_leaves_terminal_row_without_rejection_event() {
    let store = Store::open_memory().unwrap();
    insert_task_status(&store, "t-atomic-merged", TaskStatus::Merged);
    let event = TaskEvent {
        task_id: TaskId("t-atomic-merged".to_string()),
        timestamp: Local::now(),
        event_kind: EventKind::Error,
        detail: "late failure".to_string(),
        metadata: None,
    };

    let changed = store
        .complete_task_atomic(
            TaskCompletionUpdate {
                id: "t-atomic-merged",
                status: TaskStatus::Failed,
                tokens: None,
                duration_ms: 5000,
                observed_model: None,
                attribution_source: None,
                cost_usd: None,
                exit_code: Some(1),
            },
            &event,
        )
        .unwrap();

    assert!(!changed);
    let events = store.get_events("t-atomic-merged").unwrap();
    assert!(events.is_empty());
}

#[test]
fn update_principal_merge_override_persists_reason() {
    let store = Store::open_memory().unwrap();
    insert_task_status(&store, "t-override", TaskStatus::Failed);

    store
        .update_principal_merge_override("t-override", "verification command failed")
        .unwrap();

    let loaded = store.get_task("t-override").unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Failed);
    assert_eq!(
        store.get_principal_merge_override("t-override").unwrap(),
        Some("verification command failed".to_string())
    );
}

#[test]
fn get_principal_merge_override_returns_none_when_not_set() {
    let store = Store::open_memory().unwrap();
    insert_task_status(&store, "t-none", TaskStatus::Failed);

    assert_eq!(store.get_principal_merge_override("t-none").unwrap(), None);
}
