// Tests for PTY idle-liveness from raw (unparsed) agent output.
// Covers the raw-text activity signal that keeps Grok/agy-style CLIs from
// being falsely reaped, and the terminal-control exclusions that still allow
// a wedged process to be reaped.
// Depends on MonitorState, Store, and the pty_watch test task helper.

use super::MonitorState;
use super::tests::pty_task;
use crate::store::Store;
use crate::types::TaskStatus;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[test]
fn raw_unparseable_output_refreshes_progress_clock() {
    let store = Arc::new(Store::open_memory().unwrap());
    let task = pty_task("t-raw-progress", TaskStatus::Running);
    store.insert_task(&task).unwrap();
    let mut state = MonitorState::new(true, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(10);
    let mut log = tempfile::NamedTempFile::new().unwrap();

    state
        .handle_chunk(
            &crate::agent::codex::CodexAgent,
            &task.id,
            &store,
            log.as_file_mut(),
            "Building the project...\n".to_string(),
        )
        .unwrap();

    // The line cannot be parsed into an event, but it is meaningful agent
    // output and must keep a Grok/agy-style CLI from being falsely reaped.
    assert_eq!(state.event_count, 0);
    assert!(state.last_progress_time.elapsed() < Duration::from_secs(5));
}

#[test]
fn raw_unparseable_output_refreshes_clock_for_non_streaming_agents() {
    let store = Arc::new(Store::open_memory().unwrap());
    let task = pty_task("t-raw-progress-buffered", TaskStatus::Running);
    store.insert_task(&task).unwrap();
    let mut state = MonitorState::new(false, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(10);
    let mut log = tempfile::NamedTempFile::new().unwrap();

    // Grok/agy run non-streaming but still emit plain text under the PTY that
    // aid never parses; that output must keep the idle clock alive too.
    state
        .handle_chunk(
            &crate::agent::grok::GrokAgent,
            &task.id,
            &store,
            log.as_file_mut(),
            "Assembling the change set...\n".to_string(),
        )
        .unwrap();

    assert_eq!(state.event_count, 0);
    assert!(state.last_progress_time.elapsed() < Duration::from_secs(5));
}

#[test]
fn terminal_control_noise_does_not_refresh_progress_clock() {
    let store = Arc::new(Store::open_memory().unwrap());
    let task = pty_task("t-noise-progress", TaskStatus::Running);
    store.insert_task(&task).unwrap();
    let mut state = MonitorState::new(true, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(10);
    let before = state.last_progress_time;
    let mut log = tempfile::NamedTempFile::new().unwrap();

    // CSI spinner/cursor sequences strip to nothing and carry no text.
    state
        .handle_chunk(
            &crate::agent::codex::CodexAgent,
            &task.id,
            &store,
            log.as_file_mut(),
            "\x1b[?25l\x1b[?25h\n".to_string(),
        )
        .unwrap();

    assert_eq!(state.event_count, 0);
    assert_eq!(state.last_progress_time, before);
    assert!(state.last_progress_time.elapsed() > Duration::from_secs(5));
}

#[test]
fn terminal_control_noise_does_not_refresh_clock_for_non_streaming_agents() {
    let store = Arc::new(Store::open_memory().unwrap());
    let task = pty_task("t-noise-progress-buffered", TaskStatus::Running);
    store.insert_task(&task).unwrap();
    let mut state = MonitorState::new(false, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(10);
    let before = state.last_progress_time;
    let mut log = tempfile::NamedTempFile::new().unwrap();

    state
        .handle_chunk(
            &crate::agent::grok::GrokAgent,
            &task.id,
            &store,
            log.as_file_mut(),
            "\x1b[?25l\x1b[?25h\n".to_string(),
        )
        .unwrap();

    assert_eq!(state.event_count, 0);
    assert_eq!(state.last_progress_time, before);
    assert!(state.last_progress_time.elapsed() > Duration::from_secs(5));
}
