// Status-guard Store tests: waiting-row replacement, rejection events,
// and the absence of the removed AID_STATUS_GUARD escape hatch.
// Deps: Store, make_task.

use super::*;

fn rejection_events(store: &Store, id: &str) -> Vec<TaskEvent> {
    store
        .get_events(id)
        .unwrap()
        .into_iter()
        .filter(|event| event.detail.starts_with("rejected illegal status transition"))
        .collect()
}

#[test]
fn replace_waiting_task_moves_waiting_row_to_dispatch_status() {
    let store = Store::open_memory().unwrap();
    store
        .insert_waiting_task("t-rw1", "codex", "p", None, None, None, None, None, None, false, false)
        .unwrap();

    let mut task = make_task("t-rw1", AgentKind::OpenCode, TaskStatus::Running);
    task.prompt = "dispatched prompt".to_string();
    store.replace_waiting_task(&task).unwrap();

    let stored = store.get_task("t-rw1").unwrap().expect("task row");
    assert_eq!(stored.status, TaskStatus::Running);
    assert_eq!(stored.agent, AgentKind::OpenCode);
    assert_eq!(stored.prompt, "dispatched prompt");
    assert!(rejection_events(&store, "t-rw1").is_empty());
}

#[test]
fn replace_waiting_task_rejects_non_waiting_row() {
    let store = Store::open_memory().unwrap();
    store
        .insert_task(&make_task("t-rw2", AgentKind::Codex, TaskStatus::Done))
        .unwrap();

    let mut task = make_task("t-rw2", AgentKind::OpenCode, TaskStatus::Running);
    task.prompt = "must not land".to_string();
    let err = store.replace_waiting_task(&task).unwrap_err();
    assert!(err.to_string().contains("not waiting"), "{err}");

    let stored = store.get_task("t-rw2").unwrap().expect("task row");
    assert_eq!(stored.status, TaskStatus::Done);
    assert_eq!(stored.agent, AgentKind::Codex);
    assert_eq!(stored.prompt, "test prompt");
}

#[test]
fn rejected_transition_is_recorded_on_the_event_log() {
    let store = Store::open_memory().unwrap();
    store
        .insert_task(&make_task("t-rj1", AgentKind::Codex, TaskStatus::Done))
        .unwrap();

    assert!(!store.update_task_status("t-rj1", TaskStatus::Running).unwrap());

    let events = rejection_events(&store, "t-rj1");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_kind, EventKind::Error);
    assert_eq!(events[0].detail, "rejected illegal status transition: done -> running");
    let metadata = events[0].metadata.as_ref().expect("transition metadata");
    assert_eq!(metadata["from"].as_str(), Some("done"));
    assert_eq!(metadata["to"].as_str(), Some("running"));
    assert_eq!(store.get_task("t-rj1").unwrap().map(|t| t.status), Some(TaskStatus::Done));
}

#[test]
fn legal_transition_records_no_rejection_event() {
    let store = Store::open_memory().unwrap();
    store
        .insert_task(&make_task("t-rj2", AgentKind::Codex, TaskStatus::Pending))
        .unwrap();

    assert!(store.update_task_status("t-rj2", TaskStatus::Running).unwrap());
    assert!(rejection_events(&store, "t-rj2").is_empty());
}

#[test]
fn status_guard_env_var_no_longer_allows_illegal_transitions() {
    // Safety: the variable is read by nothing in the crate anymore; the test proves it.
    unsafe { std::env::set_var("AID_STATUS_GUARD", "warn") };
    let store = Store::open_memory().unwrap();
    store
        .insert_task(&make_task("t-eg1", AgentKind::Codex, TaskStatus::Merged))
        .unwrap();

    let allowed = store.update_task_status("t-eg1", TaskStatus::Running).unwrap();
    unsafe { std::env::remove_var("AID_STATUS_GUARD") };

    assert!(!allowed);
    assert_eq!(store.get_task("t-eg1").unwrap().map(|t| t.status), Some(TaskStatus::Merged));
    assert_eq!(rejection_events(&store, "t-eg1").len(), 1);
}
