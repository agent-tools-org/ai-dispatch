// Hard-cap wrap-up regression tests through the background reaper.
// Deps: parent task/spec fixtures, Store, input_signal.

use super::*;
use crate::types::EventKind;

fn poll_at(age: i64, cap: u64, agent: AgentKind) -> (Store, Task) {
    let store = Store::open_memory().expect("store");
    let mut task = task("t-wrap", TaskStatus::Running);
    task.agent = agent;
    task.read_only = false;
    task.created_at = Local::now() - Duration::seconds(age);
    store.insert_task(&task).expect("task");
    store
        .insert_event(&crate::types::TaskEvent::new(
            task.id.clone(),
            EventKind::Reasoning,
            "Agent is working".into(),
        ))
        .expect("progress");
    let mut spec = spec(task.id.as_str());
    spec.max_duration_secs = Some(cap);
    spec.idle_timeout_secs = Some(10_000);
    save_spec(&spec).expect("spec");
    check_zombie_tasks_with(&store, |_| true).expect("reap");
    (store, task)
}

#[test]
fn ninety_percent_cap_steers_commit_and_report_once_refusal_regression() {
    let _home = setup_home();
    let (store, task) = poll_at(540, 600, AgentKind::Cursor);
    let messages = store.list_messages_for_task(task.id.as_str()).expect("messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].source.as_str(), "steer");
    assert!(messages[0].content.contains("commit"));
    assert!(messages[0].content.contains("report"));
    assert!(
        store.pending_inbound_for_task(task.id.as_str())
            .expect("pending steer")
            .len() == 1
    );
    check_zombie_tasks_with(&store, |_| true).expect("reap again");
    assert_eq!(
        store
            .list_messages_for_task(task.id.as_str())
            .expect("messages")
            .len(),
        1
    );
    let events = store.get_events(task.id.as_str()).expect("events");
    let queued = events.iter().find(|event| event.detail.contains("wrap-up")).unwrap();
    assert_eq!(queued.metadata.as_ref().unwrap()["input_delivery"], "queued");
    assert!(queued.metadata.as_ref().unwrap().get("steered").is_none());
    assert_eq!(
        events
            .iter()
            .filter(|e| e.event_kind == EventKind::Milestone && e.detail.contains("wrap-up"))
            .count(),
        1
    );
    assert_eq!(
        store
            .get_task(task.id.as_str())
            .expect("task")
            .expect("task")
            .status,
        TaskStatus::Running
    );
}

#[test]
fn wrap_up_skips_early_short_and_unsupported_runs_refusal_regression() {
    for (age, cap, agent) in [
        (539, 600, AgentKind::Cursor),
        (550, 599, AgentKind::Cursor),
        (540, 600, AgentKind::Codex),
        (540, 600, AgentKind::Claude),
        (540, 600, AgentKind::Grok),
    ] {
        let _home = setup_home();
        let (store, task) = poll_at(age, cap, agent);
        assert!(
            store
                .list_messages_for_task(task.id.as_str())
                .expect("messages")
                .is_empty()
        );
        assert!(
            crate::runtime::input_signal::take_steer(task.id.as_str())
                .expect("steer")
                .is_none()
        );
    }
}

#[test]
fn read_only_reaper_queues_report_only_wrap_up() {
    let _home = setup_home();
    let store = Store::open_memory().expect("store");
    let mut task = task("t-read-only-wrap", TaskStatus::Running);
    task.agent = AgentKind::Cursor;
    task.read_only = true;
    task.created_at = Local::now() - Duration::seconds(540);
    store.insert_task(&task).expect("task");
    let mut spec = spec(task.id.as_str());
    spec.max_duration_secs = Some(600);
    spec.idle_timeout_secs = Some(10_000);
    save_spec(&spec).expect("spec");
    std::fs::write(crate::support::paths::log_path(task.id.as_str()), "agent working").expect("progress");
    check_zombie_tasks_with(&store, |_| true).expect("reap");
    assert_eq!(store.get_task(task.id.as_str()).expect("task").expect("task").status,
        TaskStatus::Running);
    let messages = store.list_messages_for_task(task.id.as_str()).expect("messages");
    assert_eq!(messages.len(), 1);
    assert!(messages[0].content.contains("stop investigating"));
    assert!(messages[0].content.contains("open questions"));
    assert!(!messages[0].content.contains("commit"));
    let events = store.get_events(task.id.as_str()).expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].detail,
        "Hard-cap wrap-up: queued request to stop investigating and write the report");
    assert!(!crate::support::paths::steer_signal_path(task.id.as_str()).exists());
}

#[test]
fn wrap_up_delivery_failure_can_recover_once_fix_regression() {
    let _home = setup_home();
    let store = Store::open_memory().expect("store");
    let mut task = task("t-wrap-recovery", TaskStatus::Running);
    task.agent = AgentKind::Cursor;
    task.read_only = false;
    store.insert_task(&task).expect("task");
    let signal = crate::support::paths::steer_signal_path(task.id.as_str());
    store.db().execute_batch(
        "CREATE TRIGGER fail_wrap_insert BEFORE INSERT ON task_messages BEGIN
           SELECT RAISE(ABORT, 'synthetic publication failure'); END;",
    ).expect("blocked publication");
    assert!(crate::cmd::steer::wrap_up_before_timeout(&store, task.id.as_str(), 540, 600).is_err());
    store.db().execute_batch("DROP TRIGGER fail_wrap_insert;").expect("unblock");
    assert!(crate::cmd::steer::wrap_up_before_timeout(&store, task.id.as_str(), 541, 600)
        .expect("retry"));
    assert_eq!(store.pending_inbound_for_task(task.id.as_str()).expect("pending steer").len(), 1);
    assert!(!crate::cmd::steer::wrap_up_before_timeout(&store, task.id.as_str(), 542, 600)
        .expect("duplicate poll"));
    assert_eq!(store.get_events(task.id.as_str()).expect("events").len(), 1);
    assert_eq!(store.list_messages_for_task(task.id.as_str()).expect("messages").len(), 1);
    assert!(!signal.exists());
}

#[test]
fn legacy_wrap_up_record_prevents_duplicate_queueing() {
    let _home = setup_home();
    let store = Store::open_memory().unwrap();
    let mut task = task("t-legacy-wrap", TaskStatus::Running);
    task.agent = AgentKind::Cursor;
    store.insert_task(&task).unwrap();
    store.insert_event(&crate::types::TaskEvent::new(
        task.id.clone(),
        EventKind::Milestone,
        "Hard-cap wrap-up: committing work in progress and writing the report requested".into(),
    ).with_metadata(Some(serde_json::json!({
            "steered": true, "hard_cap_wrap_up": true
        })))).unwrap();
    assert!(!crate::cmd::steer::wrap_up_before_timeout(&store, task.id.as_str(), 540, 600).unwrap());
    assert!(store.list_messages_for_task(task.id.as_str()).unwrap().is_empty());
    let events = store.get_events(task.id.as_str()).unwrap();
    assert_eq!(events[0].metadata.as_ref().unwrap()["steered"], true);
}

#[test]
fn worker_claim_then_reaper_keeps_one_wrap_up_request() {
    let _home = setup_home();
    let (store, task) = poll_at(0, 600, AgentKind::Cursor);
    assert!(crate::cmd::steer::wrap_up_before_timeout(
        &store, task.id.as_str(), 540, 600,
    ).expect("worker claim"));
    store.db().execute(
        "UPDATE tasks SET created_at = ?1 WHERE id = ?2",
        rusqlite::params![(Local::now() - Duration::seconds(541)).to_rfc3339(), task.id.as_str()],
    ).expect("age task");
    check_zombie_tasks_with(&store, |_| true).expect("backup reaper");
    assert_eq!(store.list_messages_for_task(task.id.as_str()).expect("messages").len(), 1);
    assert_eq!(store.get_events(task.id.as_str()).expect("events").iter()
        .filter(|event| event.detail.contains("wrap-up")).count(), 1);
}
