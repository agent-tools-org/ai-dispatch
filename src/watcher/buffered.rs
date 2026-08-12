// Buffered watcher: captures non-streaming agent output and completion state.
// Exports watch_buffered; depends on watcher milestone filtering and stderr capture.

use anyhow::Result;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, BufReader};
use tokio::process::Child;

use crate::agent::Agent;
use crate::paths;
use crate::rate_limit;
use crate::store::Store;
use crate::types::{AgentKind, CompletionInfo, TaskId, TaskStatus};

use super::extract::is_standalone_milestone_line;
use super::stderr::{drain_stderr_capture, spawn_stderr_capture};

/// Watch a non-streaming agent: buffer all output, parse at end.
pub(crate) async fn watch_buffered(
    agent: &dyn Agent,
    child: &mut Child,
    task_id: &TaskId,
    store: &Arc<Store>,
    log_path: &std::path::Path,
    output_path: Option<&std::path::Path>,
    _workgroup_id: Option<&str>,
) -> Result<CompletionInfo> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("No stdout on child process"))?;
    let mut reader = BufReader::new(stdout);
    let mut raw = Vec::new();
    let stderr_handle = spawn_stderr_capture(child, task_id);
    // Read incrementally so the background reaper can see first-token bytes
    // while the child is still alive (buffered agents emit no progress events).
    read_stdout_signaling_bytes(&mut reader, &mut raw, task_id).await?;
    let buffer = String::from_utf8_lossy(&raw).into_owned();
    persist_outputs(agent.kind(), &buffer, task_id, log_path, output_path).await?;
    if let Some(handle) = stderr_handle {
        drain_stderr_capture(handle).await;
    }
    let exit_status = child.wait().await?;
    let mut info = if exit_status.success() {
        agent.parse_completion(&buffer)
    } else {
        CompletionInfo {
            tokens: None,
            status: TaskStatus::Failed,
            model: None,
            cost_usd: None,
            exit_code: None,
        }
    };
    info.exit_code = exit_status.code();
    let task = store.get_task(task_id.as_str()).ok().flatten();
    let dispatched_model = task.as_ref().and_then(|t| t.requested_model.as_deref());
    let delivered = buffered_delivery_confirmed(agent.kind(), &buffer);
    let quota = crate::agent::stream_completion::record_quota_exhaustion_with_delivery(
        &buffer,
        agent.kind(),
        agent.rate_limit_name(),
        info.model.as_deref().or(dispatched_model),
        delivered,
    );
    if quota.should_fail() {
        info.status = TaskStatus::Failed;
    }
    if info.status == TaskStatus::Done && !quota.recorded() {
        let model = info.model.as_deref().or(dispatched_model);
        rate_limit::clear_rate_limit_for_model(&agent.kind(), agent.rate_limit_name(), model);
    }
    let event = match agent.kind() {
        AgentKind::Grok => crate::agent::grok::make_completion_event(task_id, &info),
        _ => crate::agent::gemini::make_completion_event(task_id, &info),
    };
    store.insert_event(&event)?;
    Ok(info)
}

fn buffered_delivery_confirmed(agent: AgentKind, output: &str) -> bool {
    let response = crate::agent::extract_response(agent, output);
    response.is_some_and(|text| !text.trim().is_empty())
}

async fn read_stdout_signaling_bytes(
    reader: &mut BufReader<impl tokio::io::AsyncRead + Unpin>,
    raw: &mut Vec<u8>,
    task_id: &TaskId,
) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut chunk = [0u8; 8192];
    let mut signaled = false;
    let mut signal_file: Option<tokio::fs::File> = None;
    loop {
        let n = reader.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..n]);
        if !signaled {
            tokio::fs::create_dir_all(paths::task_dir(task_id.as_str())).await?;
            let file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(paths::transcript_path(task_id.as_str()))
                .await?;
            signal_file = Some(file);
            signaled = true;
        }
        if let Some(file) = signal_file.as_mut() {
            file.write_all(&chunk[..n]).await?;
            file.flush().await?;
        }
    }
    Ok(())
}

async fn persist_outputs(
    agent: AgentKind,
    buffer: &str,
    task_id: &TaskId,
    log_path: &std::path::Path,
    output_path: Option<&std::path::Path>,
) -> Result<()> {
    let filtered: String = buffer
        .lines()
        .filter(|line| !is_standalone_milestone_line(line))
        .collect::<Vec<_>>()
        .join("\n");
    tokio::fs::write(log_path, &filtered).await?;
    let _ = tokio::fs::create_dir_all(paths::task_dir(task_id.as_str())).await;
    let _ = tokio::fs::write(paths::transcript_path(task_id.as_str()), buffer).await;
    if let Some(out_path) = output_path {
        if let Some(response) = crate::agent::extract_response(agent, buffer) {
            let response_filtered: String = response
                .lines()
                .filter(|line| !is_standalone_milestone_line(line))
                .collect::<Vec<_>>()
                .join("\n");
            tokio::fs::write(out_path, &response_filtered).await?;
        } else {
            tokio::fs::write(out_path, &filtered).await?;
        }
    }
    Ok(())
}
