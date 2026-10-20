// Deterministic stop/deadline ordering and PTY buffered failure regressions.
// Exercises monitor diagnostics and finalizers after the stop transaction wins.
// Deps: PTY monitor state, stop command and isolated task fixtures.

use super::*;
use crate::store::TaskCompletionUpdate;

fn setup() -> (
    tempfile::TempDir,
    crate::support::paths::AidHomeGuard,
    Arc<Store>,
    TaskId,
) {
    let home = tempfile::tempdir().expect("home");
    let guard = crate::support::paths::AidHomeGuard::set(home.path());
    crate::support::paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = TaskId("t-terminal-events-deadline".into());
    let task = crate::cost::test_support::task(AgentKind::Antigravity, 0, None);
    store
        .insert_task(&crate::types::Task {
            id: id.clone(),
            status: TaskStatus::Running,
            ..task
        })
        .expect("task");
    (home, guard, store, id)
}

fn assert_stopped(store: &Store, id: &TaskId, stopped: &crate::types::Task) {
    let later = store.get_task(id.as_str()).expect("query").expect("task");
    assert_eq!(later.status, TaskStatus::Stopped);
    assert_eq!(later.completed_at, stopped.completed_at);
    assert_eq!(later.duration_ms, stopped.duration_ms);
    let terminal = store
        .get_events(id.as_str())
        .expect("events")
        .into_iter()
        .filter(|event| matches!(event.event_kind, EventKind::Completion | EventKind::Error))
        .collect::<Vec<_>>();
    assert_eq!(
        terminal.len(),
        1,
        "only stop's terminal event: {terminal:?}"
    );
    assert_eq!(terminal[0].event_kind, EventKind::Completion);
    assert_eq!(terminal[0].detail, "Task stopped by user");
}

#[test]
fn stop_wins_deadline_before_pty_failure_finalization() {
    let (_home, _guard, store, id) = setup();
    crate::cmd::stop::stop(&store, id.as_str()).expect("stop");
    let stopped = store.get_task(id.as_str()).expect("query").expect("task");
    let mut state = MonitorState::new(true, None);
    state.absolute_deadline = Instant::now() - Duration::from_millis(1);
    assert!(deadline_elapsed(&mut state, &store, &id).expect("deadline"));
    super::super::finalize_output(
        &crate::runtime::watcher::idle_tests::FakeAgent,
        &id,
        &store,
        None,
        &crate::support::paths::log_path(id.as_str()),
        true,
        &portable_pty::ExitStatus::with_exit_code(1),
        &mut state,
    )
    .expect("finalize");
    assert!(
        !store
            .complete_task_atomic(
                TaskCompletionUpdate {
                    id: id.as_str(),
                    status: TaskStatus::Failed,
                    duration_ms: 500,
                    tokens: None,
                    observed_model: None,
                    attribution_source: None,
                    cost_usd: None,
                    exit_code: Some(1),
                },
                &TaskEvent {
                    task_id: id.clone(),
                    timestamp: Local::now(),
                    event_kind: EventKind::Error,
                    detail: "losing deadline".into(),
                    metadata: None,
                }
            )
            .expect("late settlement")
    );
    assert_stopped(&store, &id, &stopped);
}

#[test]
fn stop_wins_idle_failure_diagnostic() {
    let (_home, _guard, store, id) = setup();
    crate::cmd::stop::stop(&store, id.as_str()).expect("stop");
    let stopped = store.get_task(id.as_str()).expect("query").expect("task");
    crate::runtime::process::monitor::insert_hung_detected_events(&store, &id, 300, 2, None, false)
        .expect("hung diagnostic");
    assert_stopped(&store, &id, &stopped);
}

#[test]
fn buffered_pty_nonzero_exit_emits_error_instead_of_completion() {
    let (_home, _guard, store, id) = setup();
    let mut state = MonitorState::new(false, None);
    super::super::finalize_output(
        &crate::runtime::watcher::idle_tests::FakeAgent,
        &id,
        &store,
        None,
        &crate::support::paths::log_path(id.as_str()),
        false,
        &portable_pty::ExitStatus::with_exit_code(1),
        &mut state,
    )
    .expect("finalize");
    assert_eq!(state.info.status, TaskStatus::Failed);
    let events = store.get_events(id.as_str()).expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_kind, EventKind::Error);
}
