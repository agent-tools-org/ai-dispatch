// Steer delivery and hard-cap wrap-up regressions.
// Uses Store, temporary homes, and real SQLite writer locks.

use super::{run, wrap_up_before_timeout};
use crate::runtime::input_signal;
use crate::store::Store;
use crate::support::paths::AidHomeGuard;
use crate::types::{AgentKind, Task, TaskId, TaskStatus};
use chrono::Local;

fn make_task(id: &str, status: TaskStatus) -> Task {
    Task {
        agent: AgentKind::Cursor,
        prompt: "test".to_string(),
        ..crate::types::Task::test_fixture(TaskId(id.to_string()), status)
    }
}

#[test]
fn concurrent_wrap_up_pollers_queue_only_one_steer() {
    let home = tempfile::tempdir().expect("home");
    let store = Store::open(&home.path().join("aid.db")).expect("store");
    store
        .insert_task(&make_task("t-concurrent", TaskStatus::Running))
        .expect("task");
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        for _ in 0..2 {
            let barrier = &barrier;
            let path = home.path();
            scope.spawn(move || {
                let _home = AidHomeGuard::set(path);
                let store = Store::open(&path.join("aid.db")).expect("store");
                barrier.wait();
                wrap_up_before_timeout(&store, "t-concurrent", 540, 600).expect("wrap-up");
            });
        }
    });
    assert_eq!(
        store
            .list_messages_for_task("t-concurrent")
            .expect("messages")
            .len(),
        1
    );
    assert_eq!(store.get_events("t-concurrent").expect("events").len(), 1);
}

#[test]
fn legacy_wrap_up_marker_prevents_another_nudge() {
    let home = crate::support::test_env::TestHome::new();
    let path = home.path().join("aid.db");
    let store = Store::open(&path).expect("store");
    store
        .insert_task(&make_task("t-legacy", TaskStatus::Running))
        .expect("task");
    let detail = "Hard-cap wrap-up: committing work in progress and writing the report requested";
    let metadata = r#"{"steered":true,"hard_cap_wrap_up":true}"#;
    store
        .db()
        .execute(
            "INSERT INTO events (task_id, timestamp, event_type, detail, metadata)
         VALUES ('t-legacy', ?1, 'milestone', ?2, ?3)",
            rusqlite::params![Local::now().to_rfc3339(), detail, metadata],
        )
        .expect("legacy event");
    drop(store);
    let store = Store::open(&path).expect("reopened store");
    assert!(!wrap_up_before_timeout(&store, "t-legacy", 540, 600).expect("wrap-up"));
    assert!(
        store
            .list_messages_for_task("t-legacy")
            .expect("messages")
            .is_empty()
    );
    assert!(!crate::support::paths::steer_signal_path("t-legacy").exists());
    let events = store.get_events("t-legacy").expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].detail, detail);
    assert_eq!(
        events[0].metadata,
        Some(serde_json::from_str(metadata).expect("metadata"))
    );
}

#[test]
fn wrap_up_preserves_pending_operator_steer_and_skips_expired_cap() {
    let _home = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().expect("store");
    store
        .insert_task(&make_task("t-pending", TaskStatus::Running))
        .expect("task");
    input_signal::write_steer("t-pending", "operator direction").expect("steer");
    assert!(!wrap_up_before_timeout(&store, "t-pending", 540, 600).expect("wrap-up"));
    assert_eq!(
        input_signal::take_steer("t-pending")
            .expect("steer")
            .as_deref(),
        Some("operator direction")
    );
    assert!(!wrap_up_before_timeout(&store, "t-pending", 601, 600).expect("expired cap"));
    assert!(
        store
            .list_messages_for_task("t-pending")
            .expect("messages")
            .is_empty()
    );
    assert!(store.get_events("t-pending").expect("events").is_empty());
}

#[test]
fn steer_non_running_task_errors() {
    let store = Store::open_memory().unwrap();
    store
        .insert_task(&make_task("t-steer", TaskStatus::Done))
        .unwrap();
    let err = run(&store, "t-steer", "pivot").unwrap_err();
    // Steer now delegates to `aid reply`, so the error comes from the reply
    // path. Accept either phrasing so future wording tweaks don't break it.
    let msg = err.to_string();
    assert!(
        msg.contains("can only steer running tasks")
            || msg.contains("can only reply to running tasks"),
        "unexpected error: {err}"
    );
}

#[test]
fn steer_rejects_one_shot_agents_without_queuing() {
    let _temp_home = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();

    for (task_id, agent) in [
        ("t-steer-agy", AgentKind::Antigravity),
        ("t-steer-grok", AgentKind::Grok),
    ] {
        let mut task = make_task(task_id, TaskStatus::Running);
        task.agent = agent;
        store.insert_task(&task).unwrap();

        let err = run(&store, task_id, "pivot").unwrap_err();
        let message = err.to_string();
        assert!(message.contains("no steer message was queued"), "{message}");
        assert!(
            message.contains(&format!(
                "`aid stop {task_id}` then `aid retry {task_id} -f"
            )),
            "{message}"
        );
        assert!(store.list_messages_for_task(task_id).unwrap().is_empty());
        assert!(!crate::support::paths::steer_signal_path(task_id).exists());
    }
}

#[test]
fn steer_refuses_codex_without_queuing() {
    let _temp_home = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    let mut task = make_task("t-steer-codex", TaskStatus::Running);
    task.agent = AgentKind::Codex;
    store.insert_task(&task).unwrap();
    let error = run(&store, task.id.as_str(), "pivot").unwrap_err();
    assert!(error.to_string().contains("aid retry t-steer-codex -f"));
    assert!(
        store
            .list_messages_for_task(task.id.as_str())
            .unwrap()
            .is_empty()
    );
    assert!(!crate::support::paths::steer_signal_path(task.id.as_str()).exists());
}

#[test]
fn steer_explains_how_to_recover_deleted_custom_agent() {
    let _temp_home = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    let mut task = make_task("t-steer-missing-custom", TaskStatus::Running);
    task.agent = AgentKind::Custom;
    task.custom_agent_name = Some("gone".to_string());
    store.insert_task(&task).unwrap();

    let err = run(&store, task.id.as_str(), "pivot").unwrap_err();
    let message = err.to_string();
    assert!(message.contains("unavailable custom agent 'gone'"));
    assert!(message.contains("restore ~/.aid/agents/gone.toml"));
    assert!(message.contains("stop the task and retry"));
}

#[test]
fn no_op_wrap_up_reads_while_another_connection_holds_writer_lock() {
    let home = crate::support::test_env::TestHome::new();
    let path = home.path().join("aid.db");
    let store = Store::open(&path).expect("store");
    store
        .insert_task(&make_task("t-marked", TaskStatus::Running))
        .expect("task");
    store
        .insert_task(&make_task("t-done", TaskStatus::Done))
        .expect("task");
    assert!(wrap_up_before_timeout(&store, "t-marked", 540, 600).expect("first wrap-up"));
    let writer = rusqlite::Connection::open(&path).expect("writer");
    writer
        .execute_batch("BEGIN IMMEDIATE")
        .expect("writer lock");
    let start = std::time::Instant::now();
    assert!(!wrap_up_before_timeout(&store, "t-marked", 540, 600).expect("already queued"));
    for id in ["t-done", "t-missing"] {
        assert!(!super::queue_wrap_up(&store, id, false).expect("no eligible task"));
    }
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    assert_eq!(
        store
            .list_messages_for_task("t-marked")
            .expect("messages")
            .len(),
        1
    );
    assert_eq!(store.get_events("t-marked").expect("events").len(), 1);
    writer.execute_batch("ROLLBACK").expect("release writer");
}
