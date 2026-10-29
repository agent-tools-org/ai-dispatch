// Regression tests for Store status, completion and metadata mutations.
// Deps: in-memory Store, task/event types and completion payloads.

use super::mutations::*;
use crate::types::*;
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

    let event = TaskEvent::new(
        TaskId("t-atomic".to_string()),
        EventKind::Completion,
        "completed atomically".to_string(),
    );
    store
        .record_agent_completion(
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
            Some(&event),
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

    let event = TaskEvent::new(
        TaskId("t-atomic-failed".to_string()),
        EventKind::Completion,
        "completed after interrupt".to_string(),
    );
    store
        .record_agent_completion(
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
            Some(&event),
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
fn terminal_failure_cannot_be_refined_to_success() {
    let store = Store::open_memory().unwrap();
    insert_task_status(&store, "t-rescue", TaskStatus::Failed);

    let changed = store.update_task_status("t-rescue", TaskStatus::Done).unwrap();

    let task = store.get_task("t-rescue").unwrap().unwrap();
    assert!(!changed);
    assert_eq!(task.status, TaskStatus::Failed);
    assert!(store.get_events("t-rescue").unwrap().is_empty());
}

#[test]
fn complete_task_atomic_leaves_terminal_row_without_rejection_event() {
    let store = Store::open_memory().unwrap();
    insert_task_status(&store, "t-atomic-merged", TaskStatus::Merged);
    let event = TaskEvent::new(
        TaskId("t-atomic-merged".to_string()),
        EventKind::Error,
        "late failure".to_string(),
    );

    let changed = store
        .record_agent_completion(
            TaskCompletionUpdate {
                exit_code: Some(1),
                ..TaskCompletionUpdate::new("t-atomic-merged", TaskStatus::Failed, 5000)
            },
            Some(&event),
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
