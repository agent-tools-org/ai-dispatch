// Tests for PTY first-token hang detection.
// Covers the early-stall predicate before full idle timeout handling.
// Deps: MonitorState and std time controls.

use super::MonitorState;
use std::time::{Duration, Instant};

#[test]
fn first_token_hang_fires_for_streaming_without_raw_bytes() {
    let mut state = MonitorState::new(true, None);
    state.last_raw_chunk_time = Instant::now() - Duration::from_secs(181);

    assert!(state.first_token_hang_elapsed(true, Duration::from_secs(180)));

    state.event_count = 1;
    assert!(state.first_token_hang_elapsed(true, Duration::from_secs(180)));
}

#[test]
fn first_token_hang_is_inert_when_raw_bytes_keep_arriving() {
    let mut state = MonitorState::new(true, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(181);
    state.last_raw_chunk_time = Instant::now();

    assert!(!state.first_token_hang_elapsed(true, Duration::from_secs(180)));
}

#[test]
fn first_token_hang_fires_for_buffered_without_raw_bytes() {
    // grok/agy declare streaming()=false; without this they wait 2x idle
    // (t-764b2a1d: twenty minutes) for a totally silent spawn.
    let mut state = MonitorState::new(false, None);
    state.last_raw_chunk_time = Instant::now() - Duration::from_secs(181);

    assert!(state.first_token_hang_elapsed(false, Duration::from_secs(180)));
}

#[test]
fn first_token_hang_is_inert_for_buffered_after_any_raw_bytes() {
    // Silence after progress deserves the long idle budget, not first-token.
    let mut state = MonitorState::new(false, None);
    state.received_raw_bytes = true;
    state.last_raw_chunk_time = Instant::now() - Duration::from_secs(181);

    assert!(!state.first_token_hang_elapsed(false, Duration::from_secs(180)));
}

#[test]
fn first_token_hang_is_inert_after_real_progress() {
    let mut state = MonitorState::new(true, None);
    state.event_count = 2;
    state.last_raw_chunk_time = Instant::now() - Duration::from_secs(181);

    assert!(!state.first_token_hang_elapsed(true, Duration::from_secs(180)));
}
