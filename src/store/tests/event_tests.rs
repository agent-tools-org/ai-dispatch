// Event-focused Store tests.
// Exports: event query/mutation tests.
// Deps: Store, chrono.

use super::*;

#[test]
fn insert_and_get_events() {
    let store = Store::open_memory().unwrap();
    store
        .insert_task(&make_task("t-0020", AgentKind::Codex, TaskStatus::Running))
        .unwrap();

    let event = TaskEvent {
        task_id: TaskId("t-0020".to_string()),
        timestamp: Local::now(),
        event_kind: EventKind::ToolCall,
        detail: "exec: cargo test".to_string(),
        metadata: Some(serde_json::json!({"tool": "exec_command"})),
    };
    store.insert_event(&event).unwrap();

    let events = store.get_events("t-0020").unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_kind, EventKind::ToolCall);
    assert!(events[0].metadata.is_some());
}

#[test]
fn gets_latest_milestone() {
    let store = Store::open_memory().unwrap();
    store
        .insert_task(&make_task("t-0030", AgentKind::Codex, TaskStatus::Running))
        .unwrap();

    store
        .insert_event(&TaskEvent {
            task_id: TaskId("t-0030".to_string()),
            timestamp: Local::now() - chrono::Duration::seconds(2),
            event_kind: EventKind::Milestone,
            detail: "types defined".to_string(),
            metadata: None,
        })
        .unwrap();
    store
        .insert_event(&TaskEvent {
            task_id: TaskId("t-0030".to_string()),
            timestamp: Local::now() - chrono::Duration::seconds(1),
            event_kind: EventKind::ToolCall,
            detail: "cargo check".to_string(),
            metadata: None,
        })
        .unwrap();
    store
        .insert_event(&TaskEvent {
            task_id: TaskId("t-0030".to_string()),
            timestamp: Local::now(),
            event_kind: EventKind::Milestone,
            detail: "tests passing".to_string(),
            metadata: None,
        })
        .unwrap();

    let milestone = store.latest_milestone("t-0030").unwrap();
    assert_eq!(milestone.as_deref(), Some("tests passing"));
}
