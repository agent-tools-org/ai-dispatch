// Tests for PTY first-token hang detection.
// Covers the early-stall predicate before full idle timeout handling.
// Deps: MonitorState and std time controls.

use super::MonitorState;
use std::time::{Duration, Instant};

#[test]
fn first_token_hang_fires_when_progress_count_is_at_most_one() {
    let mut state = MonitorState::new(true, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(181);

    assert!(state.first_token_hang_elapsed(Duration::from_secs(180)));

    state.event_count = 1;
    assert!(state.first_token_hang_elapsed(Duration::from_secs(180)));
}

#[test]
fn first_token_hang_is_inert_after_real_progress() {
    let mut state = MonitorState::new(true, None);
    state.event_count = 2;
    state.last_progress_time = Instant::now() - Duration::from_secs(181);

    assert!(!state.first_token_hang_elapsed(Duration::from_secs(180)));
}
