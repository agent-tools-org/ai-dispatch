// Bounded accounting for rbox lock notices shared by both verify runners.
// Deps: synchronized monotonic clocks and rbox stderr markers.

use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct LockQueue(Mutex<QueueState>);

#[derive(Default)]
struct QueueState {
    waiting: Option<(Instant, Duration)>,
    queued: Duration,
}

impl LockQueue {
    pub(crate) fn observe(&self, line: &str) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(notice) = line.strip_prefix("rbox: waiting for lock (timeout ") {
            let limit = notice
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|value| value.is_finite() && *value > 0.0)
                .and_then(|value| Duration::try_from_secs_f64(value).ok());
            if let Some(limit) = limit {
                state.waiting.get_or_insert((Instant::now(), limit));
            }
        } else if line.starts_with("rbox: job ") || line.starts_with("sync:") {
            if let Some((start, limit)) = state.waiting.take() {
                state.queued += start.elapsed().min(limit);
            }
        }
    }

    pub(crate) fn elapsed(&self) -> Duration {
        let state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        state.queued
            + state
                .waiting
                .map_or(Duration::ZERO, |(start, limit)| start.elapsed().min(limit))
    }
}
