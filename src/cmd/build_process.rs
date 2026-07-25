// Cargo process supervision and build event emission for `aid build`.
// Exports: ProgressConfig and run_cargo_process().
// Deps: tokio process/io, Store events, build request and diagnostic modules.

use anyhow::{Context, Result};
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

use super::build_diag::{render_digest, BuildReport, Diagnostic, DiagnosticCollector};
use super::{BuildRequest, CargoTargetChoice};
use crate::store::Store;
use crate::types::{EventKind, TaskEvent, TaskId};

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
enum StreamEvent {
    Stdout(String),
    Stderr(String),
    Done,
}

#[derive(Debug, Default)]
struct CargoStreamState {
    collector: DiagnosticCollector,
    stderr_lines: Vec<String>,
    compiled_units: usize,
    done_streams: usize,
}

pub(crate) async fn run_cargo_process(
    store: Arc<Store>,
    request: BuildRequest,
    target: CargoTargetChoice,
    progress: ProgressConfig,
) -> Result<i32> {
    let cargo_args = request.cargo_args();
    let command = request.display_command(&target);
    let task_id = std::env::var("AID_TASK_ID").ok();
    let start = Instant::now();
    emit_event(&store, &task_id, format!("{command} started"));
    let mut child = spawn_cargo(&cargo_args, target.value.as_deref().filter(|_| !target.inherited))?;
    let stdout = child.stdout.take().context("Failed to capture cargo stdout")?;
    let stderr = child.stderr.take().context("Failed to capture cargo stderr")?;
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(pump_lines(stdout, tx.clone(), StreamEvent::Stdout));
    tokio::spawn(pump_lines(stderr, tx, StreamEvent::Stderr));

    let mut progress_state = ProgressState::new(progress);
    let mut stream_state = CargoStreamState::default();
    let status = wait_for_cargo(
        &mut child,
        &mut rx,
        &store,
        &task_id,
        &command,
        start,
        &mut progress_state,
        &mut stream_state,
    )
    .await?;
    drain_streams(&mut rx, &store, &task_id, &mut stream_state).await;
    let compiled_units = stream_state.compiled_units;
    let report = BuildReport {
        success: status.success(),
        command: command.clone(),
        elapsed: start.elapsed(),
        diagnostics: stream_state.collector.into_diagnostics(),
        stderr_lines: stream_state.stderr_lines,
    };
    println!("{}", render_digest(&report, request.include_warnings));
    emit_event(&store, &task_id, finished_detail(&command, &report, compiled_units));
    Ok(status.code().unwrap_or(1))
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
                progress_state.emit_due(start.elapsed(), store, task_id, command, stream_state.compiled_units);
            }
        }
    }
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
    fn for_tests(threshold_ms: u64, interval_ms: u64, limit: usize) -> Self {
        Self {
            threshold: Duration::from_millis(threshold_ms),
            interval: Duration::from_millis(interval_ms),
            limit,
        }
    }
}

#[derive(Debug)]
struct ProgressState {
    config: ProgressConfig,
    emitted: usize,
    next_after: Duration,
}

impl ProgressState {
    fn new(config: ProgressConfig) -> Self {
        let next_after = config.threshold;
        Self { config, emitted: 0, next_after }
    }

    fn emit_due(
        &mut self,
        elapsed: Duration,
        store: &Store,
        task_id: &Option<String>,
        command: &str,
        compiled_units: usize,
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

    fn next_detail(&mut self, elapsed: Duration, command: &str, compiled_units: usize) -> Option<String> {
        if self.emitted >= self.config.limit || elapsed < self.next_after {
            return None;
        }
        self.emitted += 1;
        self.next_after += self.config.interval;
        Some(progress_detail(command, elapsed, compiled_units))
    }
}

fn progress_detail(command: &str, elapsed: Duration, compiled_units: usize) -> String {
    format!(
        "{command} still running after {}s, {compiled_units} units compiled",
        elapsed.as_secs()
    )
}

fn spawn_cargo(cargo_args: &[String], target_dir: Option<&str>) -> Result<tokio::process::Child> {
    let mut std_cmd = std::process::Command::new("cargo");
    std_cmd.args(cargo_args);
    crate::agent::apply_cargo_target_env(&mut std_cmd, target_dir);
    std_cmd.stdout(Stdio::piped());
    std_cmd.stderr(Stdio::piped());
    Command::from(std_cmd).spawn().context("Failed to spawn cargo process")
}

async fn pump_lines<R, F>(reader: R, tx: mpsc::Sender<StreamEvent>, build_event: F)
where
    R: AsyncRead + Unpin,
    F: Fn(String) -> StreamEvent + Copy,
{
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if tx.send(build_event(line)).await.is_err() {
            return;
        }
    }
    let _ = tx.send(StreamEvent::Done).await;
}

async fn drain_streams(
    rx: &mut mpsc::Receiver<StreamEvent>,
    store: &Store,
    task_id: &Option<String>,
    stream_state: &mut CargoStreamState,
) {
    while stream_state.done_streams < 2 {
        let event = rx.recv().await;
        handle_stream_event(event, store, task_id, stream_state);
    }
}

fn handle_stream_event(
    event: Option<StreamEvent>,
    store: &Store,
    task_id: &Option<String>,
    stream_state: &mut CargoStreamState,
) {
    match event {
        Some(StreamEvent::Stdout(line)) => {
            if is_compiler_artifact_line(&line) {
                stream_state.compiled_units += 1;
            }
            if let Some(diagnostic) = stream_state.collector.push_json_line(&line) {
                emit_diagnostic_event(store, task_id, &diagnostic);
            }
        }
        Some(StreamEvent::Stderr(line)) => stream_state.stderr_lines.push(line),
        Some(StreamEvent::Done) | None => stream_state.done_streams += 1,
    }
}

fn is_compiler_artifact_line(line: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|value| value.get("reason").and_then(|reason| reason.as_str()).map(str::to_string))
        .as_deref()
        == Some("compiler-artifact")
}

fn emit_diagnostic_event(store: &Store, task_id: &Option<String>, diagnostic: &Diagnostic) {
    emit_event(store, task_id, diagnostic.event_detail());
}

fn emit_event(store: &Store, task_id: &Option<String>, detail: String) {
    if let Some(task_id) = task_id.as_ref() {
        let _ = store.insert_event(&TaskEvent {
            task_id: TaskId(task_id.clone()),
            timestamp: chrono::Local::now(),
            event_kind: EventKind::Build,
            detail,
            metadata: None,
        });
    }
}

fn finished_detail(command: &str, report: &BuildReport, compiled_units: usize) -> String {
    let errors = report.diagnostics.iter().filter(|diagnostic| diagnostic.is_error()).count();
    let warnings = report.diagnostics.len().saturating_sub(errors);
    format!("{command} finished: {errors} errors, {warnings} warnings, {compiled_units} units compiled")
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

#[cfg(test)]
#[path = "build_process_tests.rs"]
mod tests;
