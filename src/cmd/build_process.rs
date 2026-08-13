// Cargo process supervision and build event emission for `aid build`.
// Exports: ProgressConfig, run_cargo_process(), run_cargo_outcome().
// Deps: tokio process, Store events, build request/diagnostic/fallback/stream/progress.

use anyhow::{Context, Result};
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::mpsc;

use super::build_diag::BuildReport;
use crate::cmd::build_parse::evaluate_build_run;
use super::build_fallback::{
    fallback_digest_note, should_retry_with_fallback_at, target_dir_permission_blocked,
};
use super::build_stream::{
    drain_streams, emit_event, pump_lines, CargoStreamState, StreamEvent,
};
use super::{BuildRequest, CargoTargetChoice};
use crate::store::Store;

pub(crate) use super::build_progress::{ProgressConfig, ProgressState};
#[cfg(test)]
pub(super) use super::build_stream::is_compiler_artifact_line;

struct CargoAttempt {
    status: ExitStatus,
    stream_state: CargoStreamState,
    timed_out: bool,
    target: CargoTargetChoice,
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
    pub(crate) timed_out: bool,
    pub(crate) infrastructure_failure: bool,
}

pub(crate) async fn run_cargo_process(
    store: Arc<Store>,
    request: BuildRequest,
    target: CargoTargetChoice,
    progress: ProgressConfig,
) -> Result<i32> {
    let outcome =
        run_cargo_outcome(store.as_ref(), request.clone(), target, progress, None, None, &[]).await?;
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
    store: &Store,
    request: BuildRequest,
    target: CargoTargetChoice,
    progress: ProgressConfig,
    cwd: Option<&std::path::Path>,
    timeout: Option<Duration>,
    child_env: &[(String, String)],
) -> Result<CargoRunOutcome> {
    let cargo_args = request.cargo_args();
    let task_id = std::env::var("AID_TASK_ID").ok();
    let start = Instant::now();
    let command = request.display_command(&target);
    emit_event(&store, &task_id, format!("{command} started"));
    let first = run_one_attempt(
        &store,
        &task_id,
        &cargo_args,
        &command,
        &target,
        &progress,
        cwd,
        child_env,
        timeout,
    )
    .await?;
    let (attempt, command, note) = maybe_retry_after_permission_block(
        &store,
        &task_id,
        &request,
        &cargo_args,
        &progress,
        &target,
        first,
        command,
        cwd,
        timeout,
        child_env,
    )
    .await?;
    let status = attempt.status;
    let timed_out = attempt.timed_out;
    let infrastructure_failure = !status.success()
        && attempt.target.value.as_deref().is_some_and(|target_dir| {
            target_dir_permission_blocked(&attempt.stream_state.stderr_lines, target_dir)
        });
    let compiled_units = attempt.stream_state.compiled_units;
    let plain_stdout = attempt.stream_state.plain_stdout;
    let report = BuildReport {
        success: status.success(),
        command: command.clone(),
        elapsed: start.elapsed(),
        diagnostics: attempt.stream_state.collector.into_diagnostics(),
        stderr_lines: attempt.stream_state.stderr_lines,
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
        timed_out,
        infrastructure_failure,
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
    cwd: Option<&std::path::Path>,
    timeout: Option<Duration>,
    child_env: &[(String, String)],
) -> Result<(CargoAttempt, String, Option<String>)> {
    if first.timed_out {
        return Ok((first, command, None));
    }
    let Some(fallback) = should_retry_with_fallback_at(
        first.status.success(),
        &first.stream_state.stderr_lines,
        target.value.as_deref(),
        cwd,
    ) else {
        return Ok((first, command, None));
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
        cwd,
        child_env,
        timeout,
    )
    .await?;
    Ok((second, fb_command, Some(fallback_digest_note(from, &fallback))))
}

async fn run_one_attempt(
    store: &Store,
    task_id: &Option<String>,
    cargo_args: &[String],
    command: &str,
    target: &CargoTargetChoice,
    progress: &ProgressConfig,
    cwd: Option<&std::path::Path>,
    child_env: &[(String, String)],
    timeout: Option<Duration>,
) -> Result<CargoAttempt> {
    let start = Instant::now();
    // Always apply an explicit dir for non-inherited choices so a fallback can
    // override an ambient inherited CARGO_TARGET_DIR on retry.
    let mut child = spawn_cargo(
        cargo_args,
        target.value.as_deref().filter(|_| !target.inherited),
        cwd,
        child_env,
    )?;
    let stdout = child.stdout.take().context("Failed to capture cargo stdout")?;
    let stderr = child.stderr.take().context("Failed to capture cargo stderr")?;
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(pump_lines(stdout, tx.clone(), StreamEvent::Stdout));
    tokio::spawn(pump_lines(stderr, tx, StreamEvent::Stderr));
    let mut progress_state = ProgressState::new(progress.clone());
    let mut stream_state = CargoStreamState::default();
    let (status, timed_out) = wait::wait_for_cargo(
        &mut child,
        &mut rx,
        store,
        task_id,
        command,
        start,
        &mut progress_state,
        &mut stream_state,
        timeout,
    )
    .await?;
    drain_streams(&mut rx, store, task_id, &mut stream_state).await;
    Ok(CargoAttempt { status, stream_state, timed_out, target: target.clone() })
}

fn spawn_cargo(
    cargo_args: &[String],
    target_dir: Option<&str>,
    cwd: Option<&std::path::Path>,
    child_env: &[(String, String)],
) -> Result<tokio::process::Child> {
    let mut std_cmd = std::process::Command::new("cargo");
    std_cmd.args(cargo_args);
    if let Some(cwd) = cwd {
        std_cmd.current_dir(cwd);
    }
    crate::agent::apply_cargo_target_env(&mut std_cmd, target_dir);
    for (key, value) in child_env {
        std_cmd.env(key, value);
    }
    std_cmd.stdout(Stdio::piped());
    std_cmd.stderr(Stdio::piped());
    Command::from(std_cmd)
        .spawn()
        .map_err(|error| anyhow::anyhow!("Failed to spawn cargo process: {error}"))
}

#[cfg(test)]
#[path = "build_process_tests.rs"]
mod tests;

#[path = "build_process_wait.rs"]
mod wait;
