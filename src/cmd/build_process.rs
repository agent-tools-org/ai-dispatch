// Cargo process supervision and build event emission for `aid build`.
// Exports: ProgressConfig, run_cargo_process(), run_cargo_outcome().
// Deps: tokio process, Store events, build request/diagnostic/fallback/stream/progress.

use anyhow::{Context, Result};
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

use super::build_diag::BuildReport;
use crate::cmd::build_parse::evaluate_build_run;
use super::build_fallback::{fallback_digest_note, should_retry_with_fallback};
use super::build_stream::{
    drain_streams, emit_event, handle_stream_event, pump_lines, CargoStreamState, StreamEvent,
};
use super::{BuildRequest, CargoTargetChoice};
use crate::store::Store;

pub(crate) use super::build_progress::{ProgressConfig, ProgressState};
#[cfg(test)]
pub(super) use super::build_stream::is_compiler_artifact_line;

struct CargoAttempt {
    status: ExitStatus,
    stream_state: CargoStreamState,
}

/// Full cargo run result for callers that need stdout beyond the build digest.
#[derive(Debug)]
pub(crate) struct CargoRunOutcome {
    pub(crate) exit_code: i32,
    pub(crate) cargo_success: bool,
    pub(crate) command: String,
    pub(crate) elapsed: Duration,
    pub(crate) compiled_units: usize,
    pub(crate) plain_stdout: Vec<String>,
    pub(crate) report: BuildReport,
}

pub(crate) async fn run_cargo_process(
    store: Arc<Store>,
    request: BuildRequest,
    target: CargoTargetChoice,
    progress: ProgressConfig,
) -> Result<i32> {
    let outcome = run_cargo_outcome(store.clone(), request.clone(), target, progress, &[]).await?;
    let verdict = evaluate_build_run(
        &outcome.report,
        outcome.compiled_units,
        outcome.exit_code,
        request.include_warnings(),
    );
    println!("{}", verdict.digest);
    let task_id = std::env::var("AID_TASK_ID").ok();
    emit_event(&store, &task_id, verdict.event_detail);
    Ok(verdict.exit_code)
}

pub(crate) async fn run_cargo_outcome(
    store: Arc<Store>,
    request: BuildRequest,
    target: CargoTargetChoice,
    progress: ProgressConfig,
    child_env: &[(String, String)],
) -> Result<CargoRunOutcome> {
    let cargo_args = request.cargo_args();
    let task_id = std::env::var("AID_TASK_ID").ok();
    let start = Instant::now();
    let command = request.display_command(&target);
    emit_event(&store, &task_id, format!("{command} started"));
    let first =
        run_one_attempt(&store, &task_id, &cargo_args, &command, &target, &progress, child_env)
            .await?;
    let (status, stream_state, command, note) = maybe_retry_after_permission_block(
        &store,
        &task_id,
        &request,
        &cargo_args,
        &progress,
        &target,
        first,
        command,
        child_env,
    )
    .await?;
    let compiled_units = stream_state.compiled_units;
    let plain_stdout = stream_state.plain_stdout;
    let report = BuildReport {
        success: status.success(),
        command: command.clone(),
        elapsed: start.elapsed(),
        diagnostics: stream_state.collector.into_diagnostics(),
        stderr_lines: stream_state.stderr_lines,
        note,
    };
    Ok(CargoRunOutcome {
        exit_code: status.code().unwrap_or(1),
        cargo_success: status.success(),
        command,
        elapsed: start.elapsed(),
        compiled_units,
        plain_stdout,
        report,
    })
}

async fn maybe_retry_after_permission_block(
    store: &Store,
    task_id: &Option<String>,
    request: &BuildRequest,
    cargo_args: &[String],
    progress: &ProgressConfig,
    target: &CargoTargetChoice,
    first: CargoAttempt,
    command: String,
    child_env: &[(String, String)],
) -> Result<(ExitStatus, CargoStreamState, String, Option<String>)> {
    let Some(fallback) = should_retry_with_fallback(
        first.status.success(),
        &first.stream_state.stderr_lines,
        target.value.as_deref(),
    ) else {
        return Ok((first.status, first.stream_state, command, None));
    };
    let from = target.value.as_deref().unwrap_or("");
    let fb_target = CargoTargetChoice {
        value: Some(fallback.clone()),
        inherited: false,
    };
    let fb_command = request.display_command(&fb_target);
    emit_event(
        store,
        task_id,
        format!("target dir unwritable; retrying with CARGO_TARGET_DIR={fallback}"),
    );
    let second = run_one_attempt(
        store,
        task_id,
        cargo_args,
        &fb_command,
        &fb_target,
        progress,
        child_env,
    )
    .await?;
    Ok((
        second.status,
        second.stream_state,
        fb_command,
        Some(fallback_digest_note(from, &fallback)),
    ))
}

async fn run_one_attempt(
    store: &Store,
    task_id: &Option<String>,
    cargo_args: &[String],
    command: &str,
    target: &CargoTargetChoice,
    progress: &ProgressConfig,
    child_env: &[(String, String)],
) -> Result<CargoAttempt> {
    let start = Instant::now();
    // Always apply an explicit dir for non-inherited choices so a fallback can
    // override an ambient inherited CARGO_TARGET_DIR on retry.
    let mut child = spawn_cargo(
        cargo_args,
        target.value.as_deref().filter(|_| !target.inherited),
        child_env,
    )?;
    let stdout = child.stdout.take().context("Failed to capture cargo stdout")?;
    let stderr = child.stderr.take().context("Failed to capture cargo stderr")?;
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(pump_lines(stdout, tx.clone(), StreamEvent::Stdout));
    tokio::spawn(pump_lines(stderr, tx, StreamEvent::Stderr));
    let mut progress_state = ProgressState::new(progress.clone());
    let mut stream_state = CargoStreamState::default();
    let status = wait_for_cargo(
        &mut child,
        &mut rx,
        store,
        task_id,
        command,
        start,
        &mut progress_state,
        &mut stream_state,
    )
    .await?;
    drain_streams(&mut rx, store, task_id, &mut stream_state).await;
    Ok(CargoAttempt { status, stream_state })
}

async fn wait_for_cargo(
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

fn spawn_cargo(
    cargo_args: &[String],
    target_dir: Option<&str>,
    child_env: &[(String, String)],
) -> Result<tokio::process::Child> {
    let mut std_cmd = std::process::Command::new("cargo");
    std_cmd.args(cargo_args);
    crate::agent::apply_cargo_target_env(&mut std_cmd, target_dir);
    for (key, value) in child_env {
        std_cmd.env(key, value);
    }
    std_cmd.stdout(Stdio::piped());
    std_cmd.stderr(Stdio::piped());
    Command::from(std_cmd).spawn().context("Failed to spawn cargo process")
}

#[cfg(test)]
#[path = "build_process_tests.rs"]
mod tests;
