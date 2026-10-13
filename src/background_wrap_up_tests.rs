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
        .insert_event(&crate::types::TaskEvent {
            task_id: task.id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Reasoning,
            detail: "Agent is working".into(),
            metadata: None,
        })
        .expect("progress");
    let mut spec = spec(task.id.as_str());
    spec.agent_name = agent.as_str().into();
    spec.max_duration_secs = Some(cap);
    spec.idle_timeout_secs = Some(10_000);
    save_spec(&spec).expect("spec");
    check_zombie_tasks_with(&store, |_| true).expect("reap");
    (store, task)
}

#[test]
fn ninety_percent_cap_steers_commit_and_report_once_refusal_regression() {
    let _home = setup_home();
    let (store, task) = poll_at(540, 600, AgentKind::Codex);
    let messages = store.list_messages_for_task(task.id.as_str()).expect("messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].source.as_str(), "steer");
    assert!(messages[0].content.contains("commit"));
    assert!(messages[0].content.contains("report"));
    assert!(
        crate::input_signal::take_steer(task.id.as_str())
            .expect("steer")
            .is_some()
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
        (539, 600, AgentKind::Codex),
        (550, 599, AgentKind::Codex),
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
            crate::input_signal::take_steer(task.id.as_str())
                .expect("steer")
                .is_none()
        );
    }
}

#[test]
fn read_only_reaper_never_queues_wrap_up_fix_regression() {
    let _home = setup_home();
    let store = Store::open_memory().expect("store");
    let mut task = task("t-read-only-wrap", TaskStatus::Running);
    task.agent = AgentKind::Codex;
    task.read_only = true;
    task.created_at = Local::now() - Duration::seconds(540);
    store.insert_task(&task).expect("task");
    let mut spec = spec(task.id.as_str());
    spec.read_only = true;
    spec.max_duration_secs = Some(600);
    spec.idle_timeout_secs = Some(10_000);
    save_spec(&spec).expect("spec");
    std::fs::write(crate::paths::log_path(task.id.as_str()), "agent working").expect("progress");
    check_zombie_tasks_with(&store, |_| true).expect("reap");
    assert_eq!(store.get_task(task.id.as_str()).expect("task").expect("task").status,
        TaskStatus::Running);
    assert!(store.get_events(task.id.as_str()).expect("events").is_empty());
    assert!(store.list_messages_for_task(task.id.as_str()).expect("messages").is_empty());
    assert!(!crate::paths::steer_signal_path(task.id.as_str()).exists());
}

#[test]
fn wrap_up_delivery_failure_can_recover_once_fix_regression() {
    let _home = setup_home();
    let store = Store::open_memory().expect("store");
    let mut task = task("t-wrap-recovery", TaskStatus::Running);
    task.agent = AgentKind::Codex;
    task.read_only = false;
    store.insert_task(&task).expect("task");
    let signal = crate::paths::steer_signal_path(task.id.as_str());
    let parent = signal.parent().expect("parent");
    std::fs::remove_dir_all(parent).expect("remove jobs");
    std::fs::write(parent, "block steer delivery").expect("blocked parent");
    assert!(crate::cmd::steer::wrap_up_before_timeout(&store, task.id.as_str(), 540, 600).is_err());
    std::fs::remove_file(parent).expect("unblock");
    assert!(crate::cmd::steer::wrap_up_before_timeout(&store, task.id.as_str(), 541, 600)
        .expect("retry"));
    assert!(crate::input_signal::take_steer(task.id.as_str()).expect("steer").is_some());
    assert!(!crate::cmd::steer::wrap_up_before_timeout(&store, task.id.as_str(), 542, 600)
        .expect("duplicate poll"));
    assert_eq!(store.get_events(task.id.as_str()).expect("events").len(), 1);
    assert_eq!(store.list_messages_for_task(task.id.as_str()).expect("messages").len(), 1);
    assert!(!signal.exists());
}
