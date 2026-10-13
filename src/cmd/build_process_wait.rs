// Cargo child waiting with progress reporting and optional verification timeout.
// Exports: wait_for_cargo().
// Deps: build stream events, progress state, tokio child/process APIs.

use anyhow::{Context, Result};
use std::process::ExitStatus;
use std::time::{Duration, Instant};
use tokio::process::Child;
use tokio::sync::mpsc;

use super::super::build_progress::ProgressState;
use super::super::build_stream::{CargoStreamState, StreamEvent, emit_event, handle_stream_event};
use crate::store::Store;

pub(super) async fn wait_for_cargo(
    child: &mut Child,
    rx: &mut mpsc::Receiver<StreamEvent>,
    store: &Store,
    task_id: &Option<String>,
    command: &str,
    start: Instant,
    progress_state: &mut ProgressState,
    stream_state: &mut CargoStreamState,
    timeout: Option<Duration>,
) -> Result<(ExitStatus, bool)> {
    let queue = crate::remote_build::LockQueue::default();
    let poll = timeout.map_or(Duration::from_millis(100), |limit| {
        (limit / 4)
            .min(Duration::from_millis(100))
            .max(Duration::from_millis(1))
    });
    let mut ticks = tokio::time::interval(poll);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            event = rx.recv(), if stream_state.done_streams < 2 => {
                if let Some(StreamEvent::Stderr(line)) = event.as_ref() { queue.observe(line); }
                handle_stream_event(event, store, task_id, stream_state);
            }
            status = child.wait() => {
                let expired = timeout.is_some_and(|limit| start.elapsed().saturating_sub(queue.elapsed()) >= limit);
                return Ok((status.context("Failed to wait for cargo process")?, expired));
            }
            _ = ticks.tick() => {
                progress_state.emit_due(
                    start.elapsed(), store, task_id, command, stream_state.compiled_units, emit_event,
                );
                if timeout.is_some_and(|limit| start.elapsed().saturating_sub(queue.elapsed()) >= limit) {
                    child.kill().await.context("Failed to stop timed-out cargo process")?;
                    let status = child.wait().await.context("Failed to wait for timed-out cargo process")?;
                    return Ok((status, true));
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "build_process_lock_tests.rs"]
mod lock_tests;
