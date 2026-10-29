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

fn insert_history(store: &Store, now: DateTime<Local>, count: usize) {
    for (id, minutes_ago, duration_ms) in [
        ("t-done-1", 10, 120_000),
        ("t-done-2", 20, 180_000),
        ("t-done-3", 30, 240_000),
    ].into_iter().take(count) {
        store.insert_task(&make_task(
            id,
            AgentKind::Codex,
            TaskStatus::Done,
            now - Duration::minutes(minutes_ago),
            Some(duration_ms),
        )).unwrap();
    }
}

#[test]
fn estimate_eta_returns_none_without_history() {
    let (_temp, store) = crate::support::test_env::TestHome::new().store();
    let now = Local::now();
    insert_history(&store, now, 2);

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
    let (_temp, store) = crate::support::test_env::TestHome::new().store();
    let now = Local::now();
    insert_history(&store, now, 3);

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
fn progress_reports_percentage_and_caps_at_99() {
    for (case, elapsed_seconds, expected) in [
        ("test_estimate_progress_returns_percentage", 90, 50),
        ("test_estimate_progress_caps_at_99", 240, 99),
    ] {
        eprintln!("case: {case}");
        let (_temp, store) = crate::support::test_env::TestHome::new().store();
        let now = Local::now();
        insert_history(&store, now, 3);

        let running = make_task(
            "t-run",
            AgentKind::Codex,
            TaskStatus::Running,
            now - Duration::seconds(elapsed_seconds),
            None,
        );

        assert_eq!(estimate_progress_at(&running, &store, now), Some(expected));
    }
}

#[test]
fn test_estimate_progress_returns_none_without_history() {
    let (_temp, store) = crate::support::test_env::TestHome::new().store();
    let now = Local::now();
    insert_history(&store, now, 2);

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
