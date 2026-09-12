// Stderr capture, failure replay, and diagnostic notes for child processes.
// Exports capture/drain helpers, append_stderr_to_log, and failure_stderr_note.
// Deps: paths, task types, rate limits, and Tokio IO/process primitives.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Child;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{Duration, Instant};

use crate::paths;
use crate::types::{TaskId, TaskStatus};
use crate::{agent::Agent, rate_limit};

const STDERR_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) struct StderrCapture {
    handle: JoinHandle<()>,
    child_exited: oneshot::Sender<Instant>,
}

pub(super) fn spawn_stderr_capture(child: &mut Child, task_id: &TaskId) -> Option<StderrCapture> {
    let stderr = child.stderr.take()?;
    let stderr_path = paths::stderr_path(task_id.as_str());
    let (child_exited, exited) = oneshot::channel();
    let handle = tokio::spawn(capture_stderr(stderr, stderr_path, exited));
    Some(StderrCapture { handle, child_exited })
}

async fn capture_stderr(
    mut stderr: impl tokio::io::AsyncRead + Unpin,
    stderr_path: std::path::PathBuf,
    exited: oneshot::Receiver<Instant>,
) {
    let deadline = async {
        let until = exited.await.unwrap_or_else(|_| Instant::now());
        tokio::time::sleep_until(until).await;
    };
    tokio::pin!(deadline);
    let mut file = None;
    let mut chunk = [0u8; 8192];
    loop {
        let read = tokio::select! {
            biased;
            read = stderr.read(&mut chunk) => read,
            _ = &mut deadline => break,
        };
        let Ok(count) = read else { break };
        if count == 0 { break; }
        if file.is_none() {
            file = Some(tokio::fs::File::create(&stderr_path).await);
        }
        if let Some(Ok(file)) = file.as_mut() {
            let _ = file.write_all(&chunk[..count]).await;
            let _ = file.flush().await;
        }
        tokio::select! {
            biased;
            _ = &mut deadline => break,
            _ = std::future::ready(()) => {}
        }
    }
}

pub(super) async fn drain_stderr_capture(capture: StderrCapture) {
    let _ = capture.child_exited.send(Instant::now() + STDERR_DRAIN_TIMEOUT);
    let _ = capture.handle.await;
}

pub(super) async fn append_stderr_to_log(
    log_file: &mut tokio::fs::File,
    task_id: &TaskId,
) -> std::io::Result<()> {
    if let Some(stderr) = read_capped_stderr(task_id.as_str()) {
        log_file.write_all(stderr.as_bytes()).await?;
        if !stderr.ends_with('\n') {
            log_file.write_all(b"\n").await?;
        }
        log_file.flush().await?;
    }
    Ok(())
}

const MAX_PRESERVED_STDERR_BYTES: usize = 64 * 1024;

fn read_capped_stderr(task_id: &str) -> Option<String> {
    use std::io::Read;
    let file = std::fs::File::open(paths::stderr_path(task_id)).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_PRESERVED_STDERR_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    let truncated = bytes.len() > MAX_PRESERVED_STDERR_BYTES;
    bytes.truncate(MAX_PRESERVED_STDERR_BYTES);
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        text.push_str("\n[stderr truncated]");
    }
    if text.trim().is_empty() {
        return None;
    }
    Some(text)
}

pub(super) fn failure_stderr_note(
    status: TaskStatus,
    task_id: &TaskId,
    agent: &dyn Agent,
    model: Option<&str>,
) -> String {
    if status != TaskStatus::Failed {
        return String::new();
    }
    let stderr_path = paths::stderr_path(task_id.as_str());
    if !stderr_path.exists() {
        return String::new();
    }
    // stderr is a named channel (`quota_channel::Channel::CliStderr`) and the
    // reason it stays one is cursor: its spent premium pool arrives only here,
    // as `ActionRequiredError: ... You're out of usage.`, with no error envelope
    // anywhere in the stream to read it from.
    if let Ok(stderr_content) = std::fs::read_to_string(&stderr_path)
        && let Some(message) = rate_limit::refusal_on_channel(
            &stderr_content,
            agent.kind(),
            crate::quota_channel::Channel::CliStderr,
        )
    {
        rate_limit::mark_rate_limited_for_model(
            &agent.kind(),
            agent.rate_limit_name(),
            model,
            &message,
        );
    }
    format!(" — stderr: {}", stderr_path.display())
}

#[cfg(test)]
#[path = "stderr_tests.rs"]
mod regression_tests;
