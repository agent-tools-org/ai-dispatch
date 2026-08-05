// Progress rate-limiting for long-running `aid build` cargo processes.
// Exports: ProgressConfig and ProgressState.
// Deps: std time/env, Store build events.

use std::time::Duration;

use crate::store::Store;

const DEFAULT_PROGRESS_THRESHOLD_MS: u64 = 600_000;
const DEFAULT_PROGRESS_INTERVAL_MS: u64 = 600_000;
const DEFAULT_PROGRESS_LIMIT: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProgressConfig {
    threshold: Duration,
    interval: Duration,
    limit: usize,
}

#[derive(Debug)]
pub(crate) struct ProgressState {
    config: ProgressConfig,
    emitted: usize,
    next_after: Duration,
}

impl ProgressConfig {
    pub(crate) fn from_env() -> Self {
        Self {
            threshold: env_duration("AID_BUILD_PROGRESS_THRESHOLD_MS", DEFAULT_PROGRESS_THRESHOLD_MS),
            interval: env_duration("AID_BUILD_PROGRESS_INTERVAL_MS", DEFAULT_PROGRESS_INTERVAL_MS),
            limit: env_usize("AID_BUILD_PROGRESS_LIMIT", DEFAULT_PROGRESS_LIMIT),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_tests(threshold_ms: u64, interval_ms: u64, limit: usize) -> Self {
        Self {
            threshold: Duration::from_millis(threshold_ms),
            interval: Duration::from_millis(interval_ms),
            limit,
        }
    }
}

impl ProgressState {
    pub(crate) fn new(config: ProgressConfig) -> Self {
        let next_after = config.threshold;
        Self { config, emitted: 0, next_after }
    }

    pub(crate) fn emit_due(
        &mut self,
        elapsed: Duration,
        store: &Store,
        task_id: &Option<String>,
        command: &str,
        compiled_units: usize,
        emit_event: impl Fn(&Store, &Option<String>, String),
    ) {
        let Some(detail) = self.next_detail(elapsed, command, compiled_units) else {
            return;
        };
        if task_id.is_some() {
            emit_event(store, task_id, detail);
        } else {
            eprintln!("[aid] {detail}");
        }
    }

    pub(crate) fn next_detail(
        &mut self,
        elapsed: Duration,
        command: &str,
        compiled_units: usize,
    ) -> Option<String> {
        if self.emitted >= self.config.limit || elapsed < self.next_after {
            return None;
        }
        self.emitted += 1;
        self.next_after += self.config.interval;
        Some(progress_detail(command, elapsed, compiled_units))
    }

    #[cfg(test)]
    pub(crate) fn emitted(&self) -> usize {
        self.emitted
    }
}

fn progress_detail(command: &str, elapsed: Duration, compiled_units: usize) -> String {
    format!(
        "{command} still running after {}s, {compiled_units} units compiled",
        elapsed.as_secs()
    )
}

fn env_duration(name: &str, default_ms: u64) -> Duration {
    Duration::from_millis(
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(default_ms),
    )
}

fn env_usize(name: &str, default_value: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default_value)
}
