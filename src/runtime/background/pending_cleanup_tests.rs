// pending cleanup tests.
// Uses the shared test fixtures and imports from the parent test module.

use super::*;

#[test]
fn marks_stale_pending_tasks_failed() {
    let _temp = crate::support::test_env::TestHome::with_dirs();

    let store = Store::open_memory().unwrap();
    let mut task = make_task("t-aa01", TaskStatus::Pending);
    task.created_at = Local::now() - Duration::seconds(601);
    store.insert_task(&task).unwrap();

    let cleaned = check_zombie_tasks_with(&store, |_| true).unwrap();

    assert_eq!(cleaned, vec!["t-aa01".to_string()]);
    assert_eq!(
        store.get_task("t-aa01").unwrap().unwrap().status,
        TaskStatus::Failed
    );
    assert_eq!(
        store
            .get_task("t-aa01")
            .unwrap()
            .unwrap()
            .pending_reason
            .as_deref(),
        Some(PendingReason::Unknown.as_str())
    );
    let events = store.get_events("t-aa01").unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_kind, EventKind::Error);
    assert!(
        events[0]
            .detail
            .contains("Task timed out in pending state after 601s (reason: unknown)")
    );
}

#[test]
fn stale_pending_timeout_uses_rate_limited_reason() {
    let _temp = crate::support::test_env::TestHome::with_dirs();

    let store = Store::open_memory().unwrap();
    let mut task = make_task("t-aa04", TaskStatus::Pending);
    task.created_at = Local::now() - Duration::seconds(601);
    store.insert_task(&task).unwrap();
    crate::routing::rate_limit::mark_rate_limited(
        &AgentKind::Codex,
        None,
        &format!(
            "You've hit your usage limit. try again at {}.",
            crate::routing::rate_limit::test_future_recovery_time()
        ),
    );

    let changed = fail_stale_pending_task(&store, &task, Local::now(), 601).unwrap();

    assert!(changed);
    let failed_task = store.get_task("t-aa04").unwrap().unwrap();
    assert_eq!(
        failed_task.pending_reason.as_deref(),
        Some(PendingReason::RateLimited.as_str())
    );
    let events = store.get_events("t-aa04").unwrap();
    assert_eq!(
        events[0].detail,
        "Task timed out in pending state after 601s (reason: rate_limited)"
    );
}

#[test]
fn stale_pending_timeout_uses_worker_capacity_reason() {
    let _temp = crate::support::test_env::TestHome::with_dirs();

    let store = Store::open_memory().unwrap();
    let mut task = make_task("t-aa05", TaskStatus::Pending);
    task.created_at = Local::now() - Duration::seconds(601);
    store.insert_task(&task).unwrap();
    for i in 0..super::MAX_WORKERS {
        store
            .insert_task(&make_task(&format!("t-cap{i:03}"), TaskStatus::Running))
            .unwrap();
    }

    let changed = fail_stale_pending_task(&store, &task, Local::now(), 601).unwrap();

    assert!(changed);
    assert_eq!(
        store
            .get_task("t-aa05")
            .unwrap()
            .unwrap()
            .pending_reason
            .as_deref(),
        Some(PendingReason::WorkerCapacity.as_str())
    );
}

#[test]
fn stale_pending_timeout_uses_agent_starting_reason_when_agent_pid_exists() {
    let _temp = crate::support::test_env::TestHome::with_dirs();

    let store = Store::open_memory().unwrap();
    let mut task = make_task("t-a11e", TaskStatus::Pending);
    task.created_at = Local::now() - Duration::seconds(601);
    store.insert_task(&task).unwrap();
    save_spec(&BackgroundRunSpec {
        agent_pid: Some(12345),
        agent_started: None,
        ..make_spec("t-a11e")
    })
    .unwrap();

    let changed = fail_stale_pending_task(&store, &task, Local::now(), 601).unwrap();

    assert!(changed);
    assert_eq!(
        store
            .get_task("t-a11e")
            .unwrap()
            .unwrap()
            .pending_reason
            .as_deref(),
        Some(PendingReason::AgentStarting.as_str())
    );
}

#[test]
fn keeps_recent_pending_tasks_pending() {
    let store = Store::open_memory().unwrap();
    let mut task = make_task("t-aa02", TaskStatus::Pending);
    task.created_at = Local::now() - Duration::seconds(599);
    store.insert_task(&task).unwrap();

    let cleaned = cleanup_stale_pending_tasks(&store).unwrap();

    assert!(cleaned.is_empty());
    assert_eq!(
        store.get_task("t-aa02").unwrap().unwrap().status,
        TaskStatus::Pending
    );
    assert!(store.get_events("t-aa02").unwrap().is_empty());
}

#[test]
fn stale_pending_timeout_skips_tasks_that_already_moved_out_of_pending() {
    let _temp = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    let mut task = make_task("t-aa03", TaskStatus::Pending);
    task.created_at = Local::now() - Duration::seconds(601);
    store.insert_task(&task).unwrap();
    store
        .update_task_status("t-aa03", TaskStatus::Running)
        .unwrap();

    let changed = fail_stale_pending_task(&store, &task, Local::now(), 601).unwrap();

    assert!(!changed);
    assert_eq!(
        store.get_task("t-aa03").unwrap().unwrap().status,
        TaskStatus::Running
    );
    assert!(store.get_events("t-aa03").unwrap().is_empty());
}
