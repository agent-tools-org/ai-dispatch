// Tests for PTY first-token hang detection.
// Covers the early-stall predicate before full idle timeout handling.
// Deps: MonitorState and std time controls.

use super::MonitorState;
use crate::paths;
use std::time::{Duration, Instant};

#[test]
fn first_token_hang_fires_for_streaming_without_raw_bytes() {
    let mut state = MonitorState::new(true, None);
    state.last_raw_chunk_time = Instant::now() - Duration::from_secs(181);

    assert!(state.first_token_hang_elapsed(true, Duration::from_secs(180), "t-stream-hang"));

    state.event_count = 1;
    assert!(state.first_token_hang_elapsed(true, Duration::from_secs(180), "t-stream-hang"));
}

#[test]
fn first_token_hang_is_inert_when_raw_bytes_keep_arriving() {
    let mut state = MonitorState::new(true, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(181);
    state.last_raw_chunk_time = Instant::now();

    assert!(!state.first_token_hang_elapsed(true, Duration::from_secs(180), "t-stream-live"));
}

#[test]
fn first_token_hang_fires_for_buffered_without_raw_bytes_and_no_log() {
    // grok/agy declare streaming()=false. A run that writes neither PTY bytes
    // nor its debug log is dead and must be reaped after the first-token budget.
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");

    let mut state = MonitorState::new(false, None);
    state.last_raw_chunk_time = Instant::now() - Duration::from_secs(181);

    // No log file written — nothing at the agent_log_path.
    assert!(
        state.first_token_hang_elapsed(false, Duration::from_secs(180), "t-buf-dead"),
        "a run that writes nothing must be reaped"
    );
}

#[test]
fn first_token_hang_is_inert_for_buffered_with_growing_log() {
    // grok runs --debug-file pointing at agent_log_path. A run whose log is
    // growing is alive even if the PTY is silent for the whole first-token window.
    // Replay of t-73b69cde and t-24f12f38: both died at exactly 183s.
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().expect("ensure dirs");
    std::fs::create_dir_all(paths::task_dir("t-buf-live")).expect("task dir");

    let agent_log = paths::agent_log_path("t-buf-live");
    std::fs::write(&agent_log, "grok debug output...\n").expect("write log");

    let mut state = MonitorState::new(false, None);
    // Make the log look written after MonitorState was created.
    state.start_system_time = std::time::SystemTime::now() - Duration::from_secs(200);
    state.last_raw_chunk_time = Instant::now() - Duration::from_secs(181);

    assert!(
        !state.first_token_hang_elapsed(false, Duration::from_secs(180), "t-buf-live"),
        "a run whose log is growing must survive the first-token window"
    );
}

#[test]
fn first_token_hang_is_inert_for_buffered_after_any_raw_bytes() {
    // Silence after progress deserves the long idle budget, not first-token.
    let mut state = MonitorState::new(false, None);
    state.received_raw_bytes = true;
    state.last_raw_chunk_time = Instant::now() - Duration::from_secs(181);

    assert!(!state.first_token_hang_elapsed(false, Duration::from_secs(180), "t-buf-raw"));
}

#[test]
fn first_token_hang_is_inert_after_real_progress() {
    let mut state = MonitorState::new(true, None);
    state.event_count = 2;
    state.last_raw_chunk_time = Instant::now() - Duration::from_secs(181);

    assert!(!state.first_token_hang_elapsed(true, Duration::from_secs(180), "t-stream-progress"));
}
