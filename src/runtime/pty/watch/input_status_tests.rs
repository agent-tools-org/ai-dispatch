// Regressions for truthful automatic-nudge queue status.
// Exercises monitor idle handling before any PTY write or consumption.
// Deps: MonitorState, Store, task fixtures, and clock durations.
use super::*;

#[test]
fn auto_unstick_queue_status_audit_regression() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = super::tests::pty_task("t-auto-queue", TaskStatus::Running);
    task.agent = AgentKind::Cursor;
    store.insert_task(&task).unwrap();
    let mut state = MonitorState::new(true, None);
    state.last_progress_time = Instant::now() - state.idle_detector.nudge_after;
    state.maybe_handle_idle(&store, &task.id, true).unwrap();
    let messages = store.list_messages_for_task(task.id.as_str()).unwrap();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].delivered_at.is_none());
    let events = store.get_events(task.id.as_str()).unwrap();
    let event = events
        .iter()
        .find(|event| event.detail.contains("Auto-nudge"))
        .unwrap();
    assert_eq!(event.detail, "Auto-nudge queued");
    assert_eq!(event.metadata.as_ref().unwrap()["input_delivery"], "queued");
}

#[test]
fn steer_pty_write_status_audit_regression() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store) = crate::support::test_env::TestHome::new().shared_store();
    let mut task = super::tests::pty_task("t-steer-stage", TaskStatus::Running);
    task.agent = AgentKind::Cursor;
    store.insert_task(&task).unwrap();
    store
        .insert_message(
            task.id.as_str(),
            crate::types::MessageDirection::In,
            "pivot",
            crate::types::MessageSource::Steer,
        )
        .unwrap();
    input_signal::write_steer(task.id.as_str(), "pivot").unwrap();
    let command = ["/bin/sh", "-c", "read line"].map(String::from).to_vec();
    let mut bridge = PtyBridge::spawn(&command, None, vec![]).unwrap();
    MonitorState::new(true, None)
        .maybe_forward_steer(&mut bridge, &store, &task.id)
        .unwrap();
    assert!(bridge.wait().unwrap().success());
    let event = store.get_events(task.id.as_str()).unwrap().remove(0);
    let metadata = event.metadata.unwrap();
    assert_eq!(metadata["input_delivery"], "written");
    assert_eq!(metadata["consumption"], "unconfirmed");
    assert!(metadata.get("steered").is_none());
    assert!(metadata.get("delivered").is_none());
}
