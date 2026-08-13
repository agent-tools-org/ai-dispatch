// Cargo child waiting with progress reporting and optional verification timeout.
// Exports: wait_for_cargo().
// Deps: build stream events, progress state, tokio child/process APIs.

use anyhow::{Context, Result};
use std::process::ExitStatus;
use std::time::{Duration, Instant};
use tokio::process::Child;
use tokio::sync::mpsc;

use super::super::build_progress::ProgressState;
use super::super::build_stream::{emit_event, handle_stream_event, CargoStreamState, StreamEvent};
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
    let wait = wait_for_cargo_until_exit(
        child,
        rx,
        store,
        task_id,
        command,
        start,
        progress_state,
        stream_state,
    );
    if let Some(timeout) = timeout {
        match tokio::time::timeout(timeout, wait).await {
            Ok(status) => return Ok((status?, false)),
            Err(_) => {
                child.kill().await.context("Failed to stop timed-out cargo process")?;
                let status = child.wait().await.context("Failed to wait for timed-out cargo process")?;
                return Ok((status, true));
            }
        }
    }
    Ok((wait.await?, false))
}

async fn wait_for_cargo_until_exit(
    child: &mut Child,
    rx: &mut mpsc::Receiver<StreamEvent>,
    store: &Store,
    task_id: &Option<String>,
    command: &str,
    start: Instant,
    progress_state: &mut ProgressState,
    stream_state: &mut CargoStreamState,
) -> Result<ExitStatus> {
    loop {
        tokio::select! {
            event = rx.recv(), if stream_state.done_streams < 2 => {
                handle_stream_event(event, store, task_id, stream_state);
            }
            status = child.wait() => {
                return status.context("Failed to wait for cargo process");
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                progress_state.emit_due(
                    start.elapsed(),
                    store,
                    task_id,
                    command,
                    stream_state.compiled_units,
                    emit_event,
                );
            }
        }
    }
}
