// Tests for PTY idle hang detection with buffered-agent log liveness.
// Deps: MonitorState, paths::agent_log_path, AidHomeGuard.

use super::MonitorState;
use crate::paths;
use std::time::{Duration, Instant};

#[test]
fn idle_hang_fires_for_streaming_when_progress_stale() {
    let mut state = MonitorState::new(true, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(601);

    assert!(state.idle_hang_elapsed(true, Duration::from_secs(600), "t-stream-idle"));
}

#[test]
fn idle_hang_is_inert_for_streaming_with_fresh_progress() {
    let mut state = MonitorState::new(true, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(10);

    assert!(!state.idle_hang_elapsed(true, Duration::from_secs(600), "t-stream-fresh"));
}

#[test]
fn idle_hang_fires_for_buffered_without_log_growth() {
    // Replay of t-54c4560a shape: PTY silent, progress clock stale, no recent
    // agent.log write → idle watchdog must still reap.
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");

    let mut state = MonitorState::new(false, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(601);

    assert!(
        state.idle_hang_elapsed(false, Duration::from_secs(600), "t-buf-idle-dead"),
        "buffered agent with no log growth must be reaped"
    );
}

#[test]
fn idle_hang_is_inert_for_buffered_with_log_growth_in_window() {
    // grok/agy write agent.log while the PTY stays silent. A log written
    // inside the idle window is growth, not mere existence — keep alive.
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    std::fs::create_dir_all(paths::task_dir("t-buf-idle-live")).expect("task dir");

    let agent_log = paths::agent_log_path("t-buf-idle-live");
    std::fs::write(&agent_log, "tool call in flight...\n").expect("write log");

    let mut state = MonitorState::new(false, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(601);

    assert!(
        !state.idle_hang_elapsed(false, Duration::from_secs(600), "t-buf-idle-live"),
        "buffered agent whose log grew inside the idle window must survive"
    );
}

#[test]
fn idle_hang_fires_for_buffered_when_log_is_stale() {
    // A log that merely exists from earlier in the run is not proof of life.
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    std::fs::create_dir_all(paths::task_dir("t-buf-idle-stale")).expect("task dir");

    let agent_log = paths::agent_log_path("t-buf-idle-stale");
    std::fs::write(&agent_log, "wrote once then hung\n").expect("write log");
    let stale = std::time::SystemTime::now() - Duration::from_secs(700);
    std::fs::File::open(&agent_log)
        .expect("open")
        .set_modified(stale)
        .expect("mtime");

    let mut state = MonitorState::new(false, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(601);

    assert!(
        state.idle_hang_elapsed(false, Duration::from_secs(600), "t-buf-idle-stale"),
        "stale agent.log must not keep a hung buffered agent alive"
    );
}

#[test]
fn idle_hang_streaming_ignores_agent_log() {
    // Streaming idle semantics stay progress-clock only even if a log exists.
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    std::fs::create_dir_all(paths::task_dir("t-stream-log")).expect("task dir");
    std::fs::write(
        paths::agent_log_path("t-stream-log"),
        "unrelated log growth\n",
    )
    .expect("write log");

    let mut state = MonitorState::new(true, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(601);

    assert!(
        state.idle_hang_elapsed(true, Duration::from_secs(600), "t-stream-log"),
        "streaming agents must not gain idle immunity from agent.log"
    );
}
