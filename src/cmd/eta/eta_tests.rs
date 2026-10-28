// ETA and progress estimation tests.
// Deps: parent estimation helpers, in-memory task history.

use super::*;
use crate::store::Store;
use crate::support::paths::AidHomeGuard;
use crate::types::{AgentKind, TaskId};
use chrono::Duration;
use tempfile::TempDir;

fn make_task(
    id: &str,
    agent: AgentKind,
    status: TaskStatus,
    created_at: DateTime<Local>,
    duration_ms: Option<i64>,
) -> Task {
    Task {
        agent,
        prompt: "test prompt".to_string(),
        duration_ms,
        created_at,
        ..crate::types::Task::test_fixture(TaskId(id.to_string()), status)
    }
}

fn isolated_store() -> (TempDir, AidHomeGuard, Store) {
    let temp = TempDir::new().unwrap();
    let guard = AidHomeGuard::set(temp.path());
    let store = Store::open_memory().unwrap();
    (temp, guard, store)
}

#[test]
fn estimate_eta_returns_none_without_history() {
    let (_temp, _guard, store) = isolated_store();
    let now = Local::now();
    store
        .insert_task(&make_task(
            "t-done-1",
            AgentKind::Codex,
            TaskStatus::Done,
            now - Duration::minutes(10),
            Some(120_000),
        ))
        .unwrap();
    store
        .insert_task(&make_task(
            "t-done-2",
            AgentKind::Codex,
            TaskStatus::Done,
            now - Duration::minutes(20),
            Some(180_000),
        ))
        .unwrap();

    let running = make_task(
        "t-run",
        AgentKind::Codex,
        TaskStatus::Running,
        now - Duration::seconds(30),
        None,
    );

    assert_eq!(estimate_eta_at(&running, &store, now), None);
}

#[test]
fn estimate_eta_returns_remaining_time() {
    let (_temp, _guard, store) = isolated_store();
    let now = Local::now();
    for (id, minutes_ago, duration_ms) in [
        ("t-done-1", 10, 120_000),
        ("t-done-2", 20, 180_000),
        ("t-done-3", 30, 240_000),
    ] {
        store
            .insert_task(&make_task(
                id,
                AgentKind::Codex,
                TaskStatus::Done,
                now - Duration::minutes(minutes_ago),
                Some(duration_ms),
            ))
            .unwrap();
    }

    let running = make_task(
        "t-run",
        AgentKind::Codex,
        TaskStatus::Running,
        now - Duration::seconds(60),
        None,
    );

    assert_eq!(estimate_eta_at(&running, &store, now), Some("~2m".to_string()));
}

#[test]
fn test_estimate_progress_returns_percentage() {
    let (_temp, _guard, store) = isolated_store();
    let now = Local::now();
    for (id, minutes_ago, duration_ms) in [
        ("t-done-1", 10, 120_000),
        ("t-done-2", 20, 180_000),
        ("t-done-3", 30, 240_000),
    ] {
        store
            .insert_task(&make_task(
                id,
                AgentKind::Codex,
                TaskStatus::Done,
                now - Duration::minutes(minutes_ago),
                Some(duration_ms),
            ))
            .unwrap();
    }

    let running = make_task(
        "t-run",
        AgentKind::Codex,
        TaskStatus::Running,
        now - Duration::seconds(90),
        None,
    );

    assert_eq!(estimate_progress_at(&running, &store, now), Some(50));
}

#[test]
fn test_estimate_progress_caps_at_99() {
    let (_temp, _guard, store) = isolated_store();
    let now = Local::now();
    for (id, minutes_ago, duration_ms) in [
        ("t-done-1", 10, 120_000),
        ("t-done-2", 20, 180_000),
        ("t-done-3", 30, 240_000),
    ] {
        store
            .insert_task(&make_task(
                id,
                AgentKind::Codex,
                TaskStatus::Done,
                now - Duration::minutes(minutes_ago),
                Some(duration_ms),
            ))
            .unwrap();
    }

    let running = make_task(
        "t-run",
        AgentKind::Codex,
        TaskStatus::Running,
        now - Duration::seconds(240),
        None,
    );

    assert_eq!(estimate_progress_at(&running, &store, now), Some(99));
}

#[test]
fn test_estimate_progress_returns_none_without_history() {
    let (_temp, _guard, store) = isolated_store();
    let now = Local::now();
    store
        .insert_task(&make_task(
            "t-done-1",
            AgentKind::Codex,
            TaskStatus::Done,
            now - Duration::minutes(10),
            Some(120_000),
        ))
        .unwrap();
    store
        .insert_task(&make_task(
            "t-done-2",
            AgentKind::Codex,
            TaskStatus::Done,
            now - Duration::minutes(20),
            Some(180_000),
        ))
        .unwrap();

    let running = make_task(
        "t-run",
        AgentKind::Codex,
        TaskStatus::Running,
        now - Duration::seconds(30),
        None,
    );

    assert_eq!(estimate_progress_at(&running, &store, now), None);
}

#[test]
fn format_eta_formats_correctly() {
    assert_eq!(format_eta(59_000), "~59s");
    assert_eq!(format_eta(60_000), "~1m");
    assert_eq!(format_eta(3_720_000), "~1h2m");
}
