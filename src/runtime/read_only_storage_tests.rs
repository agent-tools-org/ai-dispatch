// Private provenance persistence regressions for read-only and ordinary tasks.
// Checks event isolation, durable replacement and opening databases without the new table.

use super::*;

fn task(store: &Store, read_only: bool) -> TaskId {
    let id = TaskId("t-private-provenance".into());
    store
        .insert_waiting_task(
            id.as_str(),
            "codex",
            "audit",
            None,
            None,
            None,
            None,
            None,
            None,
            read_only,
            false,
        )
        .unwrap();
    id
}

#[test]
fn non_read_only_attempts_write_no_provenance_or_events() {
    let repo = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let store = Store::open_memory().unwrap();
    let id = task(&store, false);
    let before: i64 = store
        .db()
        .query_row("SELECT total_changes()", [], |row| row.get(0))
        .unwrap();
    reset(&store, &id).unwrap();
    record(&store, &id, &command(repo.path(), scratch.path(), false), "exec").unwrap();
    record(&store, &id, &Command::new("claude"), "exec").unwrap();
    let after: i64 = store
        .db()
        .query_row("SELECT total_changes()", [], |row| row.get(0))
        .unwrap();
    assert_eq!(before, after);
    assert!(store.read_only_launch(id.as_str()).unwrap().is_none());
    assert!(store.get_events(id.as_str()).unwrap().is_empty());
}

#[test]
fn read_only_attempts_preserve_user_events_and_replace_private_proof() {
    let repo = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let store = Store::open_memory().unwrap();
    let id = task(&store, true);
    store
        .insert_event(&TaskEvent {
            task_id: id.clone(),
            timestamp: chrono::Local::now(),
            event_kind: EventKind::Milestone,
            detail: "Visible agent progress".into(),
            metadata: None,
        })
        .unwrap();
    let cmd = command(repo.path(), scratch.path(), false);
    record(&store, &id, &cmd, "app-server").unwrap();
    assert!(enforced(&store, &id, repo.path().to_str().unwrap()).unwrap());
    reset(&store, &id).unwrap();
    assert_eq!(
        store.read_only_launch(id.as_str()).unwrap().as_deref(),
        Some("null")
    );
    assert!(!enforced(&store, &id, repo.path().to_str().unwrap()).unwrap());
    record(&store, &id, &cmd, "exec").unwrap();
    record(&store, &id, &Command::new("custom-agent"), "exec").unwrap();
    assert!(!enforced(&store, &id, repo.path().to_str().unwrap()).unwrap());
    let events = store.get_events(id.as_str()).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].detail, "Visible agent progress");
    let count: i64 = store
        .db()
        .query_row("SELECT count(*) FROM task_read_only_launch", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn private_read_only_provenance_survives_reopen_and_cascades_with_task_deletion() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let path = home.path().join("tasks.sqlite");
    let store = Store::open(&path).unwrap();
    let id = task(&store, true);
    record(&store, &id, &command(repo.path(), scratch.path(), true), "exec").unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    assert!(enforced(&store, &id, repo.path().to_str().unwrap()).unwrap());
    assert!(store.get_events(id.as_str()).unwrap().is_empty());
    store
        .db()
        .execute("DELETE FROM tasks WHERE id = ?1", [id.as_str()])
        .unwrap();
    assert!(store.read_only_launch(id.as_str()).unwrap().is_none());
}

#[test]
fn older_database_initializes_private_table_without_trusting_event_provenance() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let path = home.path().join("old.sqlite");
    let store = Store::open(&path).unwrap();
    let id = task(&store, true);
    let proof = profile(&command(repo.path(), scratch.path(), false)).unwrap();
    store
        .insert_event(&TaskEvent {
            task_id: id.clone(),
            timestamp: chrono::Local::now(),
            event_kind: EventKind::Setup,
            detail: "Read-only launch provenance".into(),
            metadata: Some(serde_json::json!({
                "read_only_launch": {"transport": "exec", "profile": proof},
            })),
        })
        .unwrap();
    store
        .db()
        .execute_batch("DROP TABLE task_read_only_launch")
        .unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    assert!(!enforced(&store, &id, repo.path().to_str().unwrap()).unwrap());
    assert!(store.read_only_launch(id.as_str()).unwrap().is_none());
}
