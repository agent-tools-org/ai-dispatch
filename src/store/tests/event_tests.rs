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

#[test]
fn latest_observed_models_keep_the_newest_model_per_agent() {
    let store = Store::open_memory().unwrap();
    let mut older = make_task("t-model-old", AgentKind::Codex, TaskStatus::Done);
    older.created_at -= chrono::Duration::minutes(1);
    older.observed_model = Some("gpt-old".to_string());
    let mut newer = make_task("t-model-new", AgentKind::Codex, TaskStatus::Done);
    newer.observed_model = Some("gpt-new".to_string());
    store.insert_task(&older).unwrap();
    store.insert_task(&newer).unwrap();

    let models = store.latest_observed_models().unwrap();
    assert_eq!(models.get("codex").map(String::as_str), Some("gpt-new"));
}

#[test]
fn latest_events_batch_returns_only_three_events_per_task() {
    let store = Store::open_memory().unwrap();
    store.insert_task(&make_task("t-events", AgentKind::Codex, TaskStatus::Running)).unwrap();
    for index in 0..5 {
        store
            .insert_event(&TaskEvent {
                task_id: TaskId("t-events".to_string()),
                timestamp: Local::now(),
                event_kind: EventKind::ToolCall,
                detail: format!("event-{index}"),
                metadata: None,
            })
            .unwrap();
    }

    let events = store.latest_events_three_batch(&["t-events"]).unwrap();
    let details: Vec<_> = events["t-events"].iter().map(|event| event.detail.as_str()).collect();
    assert_eq!(details, ["event-2", "event-3", "event-4"]);
}

#[test]
fn started_at_batch_uses_the_earliest_start_event_when_column_is_empty() {
    let store = Store::open_memory().unwrap();
    store.insert_task(&make_task("t-started-event", AgentKind::Codex, TaskStatus::Done)).unwrap();
    let started = Local::now() - chrono::Duration::minutes(2);
    store
        .insert_event(&TaskEvent {
            task_id: TaskId("t-started-event".to_string()),
            timestamp: started,
            event_kind: EventKind::Setup,
            detail: "started".to_string(),
            metadata: None,
        })
        .unwrap();

    let values = store.started_at_batch(&["t-started-event"]).unwrap();
    assert_eq!(values.get("t-started-event").map(String::as_str), Some(started.to_rfc3339().as_str()));
}
