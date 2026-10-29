// worker cleanup tests.
// Uses the shared test fixtures and imports from the parent test module.

use super::*;

#[test]
fn marks_running_background_tasks_failed_when_worker_is_missing() {
    let (_temp, store) = crate::support::test_env::TestHome::with_dirs().store();
    store
        .insert_task(&make_task("t-1a1a", TaskStatus::Running))
        .unwrap();
    store
        .insert_task(&make_task("t-2b2b", TaskStatus::Running))
        .unwrap();
    store
        .insert_task(&make_task("t-3c3c", TaskStatus::Running))
        .unwrap();
    save_spec(&make_spec("t-1a1a")).unwrap();
    save_spec(&make_spec("t-2b2b")).unwrap();

    let cleaned = check_zombie_tasks_with(&store, |pid| pid == 101).unwrap();

    assert_eq!(cleaned, vec!["t-2b2b".to_string()]);
    assert_eq!(
        store.get_task("t-1a1a").unwrap().unwrap().status,
        TaskStatus::Running
    );
    assert_eq!(
        store.get_task("t-2b2b").unwrap().unwrap().status,
        TaskStatus::Failed
    );
    assert_eq!(
        store.get_task("t-3c3c").unwrap().unwrap().status,
        TaskStatus::Running
    );

    let events = store.get_events("t-2b2b").unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_kind, EventKind::Error);
    assert!(events[0].detail.starts_with(ZOMBIE_FAILURE_DETAIL));
    assert!(events[0].detail.contains("worker.log"));

    let stderr = std::fs::read_to_string(paths::stderr_path("t-2b2b")).unwrap();
    assert_eq!(stderr.trim(), events[0].detail);
}

#[test]
fn reaps_alive_worker_when_events_are_stale_beyond_idle_margin() {
    let (_temp, store) = crate::support::test_env::TestHome::with_dirs().store();
    let task = make_task("t-wedged", TaskStatus::Running);
    store.insert_task(&task).unwrap();
    save_spec(&BackgroundRunSpec {
        worker_pid: Some(202),
        idle_timeout_secs: Some(30),
        ..make_spec("t-wedged")
    })
    .unwrap();
    insert_event(&store, "t-wedged", 61);

    let cleaned = check_zombie_tasks_with(&store, |pid| pid == 202).unwrap();

    assert_eq!(cleaned, vec!["t-wedged".to_string()]);
    assert_eq!(
        store.get_task("t-wedged").unwrap().unwrap().status,
        TaskStatus::Failed
    );
    let events = store.get_events("t-wedged").unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.detail.contains("monitor wedged"))
    );
    assert!(events.iter().any(|event| event.detail == "hung_detected"));
    let completions = crate::task::notify::read_recent(10).unwrap();
    assert!(completions.contains("\"task_id\":\"t-wedged\""));
    let log = std::fs::read_to_string(paths::log_path("t-wedged")).unwrap();
    assert!(log.contains("=== AID TASK t-wedged FAILED (hung detected (monitor wedged):"));
}

#[test]
fn keeps_alive_worker_when_events_are_recent_within_idle_margin() {
    let (_temp, store) = crate::support::test_env::TestHome::with_dirs().store();
    let task = make_task("t-healthy", TaskStatus::Running);
    store.insert_task(&task).unwrap();
    save_spec(&BackgroundRunSpec {
        worker_pid: Some(202),
        idle_timeout_secs: Some(30),
        ..make_spec("t-healthy")
    })
    .unwrap();
    insert_event(&store, "t-healthy", 20);

    let cleaned = check_zombie_tasks_with(&store, |pid| pid == 202).unwrap();

    assert!(cleaned.is_empty());
    assert_eq!(
        store.get_task("t-healthy").unwrap().unwrap().status,
        TaskStatus::Running
    );
}

#[test]
#[cfg(unix)]
fn skips_awaiting_input_live_worker_even_when_events_are_stale() {
    let _permit = test_subprocess::acquire();
    let (_temp, store) = crate::support::test_env::TestHome::with_dirs().store();
    let task = make_task("t-awaiting", TaskStatus::AwaitingInput);
    store.insert_task(&task).unwrap();
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let worker_pid = child.id();
    save_spec(&BackgroundRunSpec {
        worker_pid: Some(worker_pid),
        idle_timeout_secs: Some(30),
        ..make_spec("t-awaiting")
    })
    .unwrap();
    insert_event(&store, "t-awaiting", 61);

    let cleaned = check_zombie_tasks_with(&store, |pid| pid == worker_pid).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    let still_running = child.try_wait().unwrap().is_none();
    let _ = child.kill();
    let _ = child.wait();

    assert!(cleaned.is_empty());
    assert_eq!(
        store.get_task("t-awaiting").unwrap().unwrap().status,
        TaskStatus::AwaitingInput
    );
    assert!(still_running);
}

#[test]
fn reaps_running_task_kept_alive_only_by_idle_bookkeeping_events() {
    let (_temp, store) = crate::support::test_env::TestHome::with_dirs().store();
    let task = make_task("t-idle-bookkeeping", TaskStatus::Running);
    store.insert_task(&task).unwrap();
    save_spec(&BackgroundRunSpec {
        worker_pid: Some(202),
        idle_timeout_secs: Some(30),
        ..make_spec("t-idle-bookkeeping")
    })
    .unwrap();
    insert_event(&store, "t-idle-bookkeeping", 61);
    insert_idle_escalation_event(&store, "t-idle-bookkeeping", 1);

    let cleaned = check_zombie_tasks_with(&store, |pid| pid == 202).unwrap();

    assert_eq!(cleaned, vec!["t-idle-bookkeeping".to_string()]);
    assert_eq!(
        store
            .get_task("t-idle-bookkeeping")
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Failed
    );
}

#[test]
fn reaps_running_task_kept_alive_only_by_acked_reply_bookkeeping() {
    let (_temp, store) = crate::support::test_env::TestHome::with_dirs().store();
    let task = make_task("t-acked-bookkeeping", TaskStatus::Running);
    store.insert_task(&task).unwrap();
    save_spec(&BackgroundRunSpec {
        worker_pid: Some(202),
        idle_timeout_secs: Some(30),
        ..make_spec("t-acked-bookkeeping")
    })
    .unwrap();
    insert_event(&store, "t-acked-bookkeeping", 61);
    store
        .insert_event(&TaskEvent::at(
            TaskId("t-acked-bookkeeping".to_string()),
            EventKind::Reasoning,
            "Acked reply".to_string(),
            Local::now() - Duration::seconds(1),
        ).with_metadata(Some(serde_json::json!({ "acked_reply": true }))))
        .unwrap();

    let cleaned = check_zombie_tasks_with(&store, |pid| pid == 202).unwrap();

    assert_eq!(cleaned, vec!["t-acked-bookkeeping".to_string()]);
    assert_eq!(
        store
            .get_task("t-acked-bookkeeping")
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Failed
    );
}

#[test]
fn reaps_running_task_kept_alive_only_by_nudge_echo_reasoning() {
    let (_temp, store) = crate::support::test_env::TestHome::with_dirs().store();
    let task = make_task("t-nudge-echo-reaper", TaskStatus::Running);
    store.insert_task(&task).unwrap();
    save_spec(&BackgroundRunSpec {
        worker_pid: Some(202),
        idle_timeout_secs: Some(30),
        ..make_spec("t-nudge-echo-reaper")
    })
    .unwrap();
    insert_event(&store, "t-nudge-echo-reaper", 61);
    store
        .insert_event(&TaskEvent::at(
            TaskId("t-nudge-echo-reaper".to_string()),
            EventKind::Reasoning,
            crate::task::unstick::default_nudge_message(),
            Local::now() - Duration::seconds(1),
        ))
        .unwrap();

    let cleaned = check_zombie_tasks_with(&store, |pid| pid == 202).unwrap();

    assert_eq!(cleaned, vec!["t-nudge-echo-reaper".to_string()]);
    assert_eq!(
        store
            .get_task("t-nudge-echo-reaper")
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Failed
    );
}
