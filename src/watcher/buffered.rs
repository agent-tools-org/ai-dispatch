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
    let mut buffer = String::new();
    let stderr_handle = spawn_stderr_capture(child, task_id);
    reader.read_to_string(&mut buffer).await?;
    persist_outputs(&buffer, task_id, log_path, output_path).await?;
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
    let quota = crate::agent::stream_completion::record_quota_exhaustion(
        &buffer,
        agent.kind(),
        info.model.as_deref().or(dispatched_model),
    );
    if quota.should_fail() {
        info.status = TaskStatus::Failed;
    }
    if info.status == TaskStatus::Done && !quota.recorded() {
        let model = info.model.as_deref().or(dispatched_model);
        rate_limit::clear_rate_limit_for_model(&agent.kind(), model);
    }
    let event = match agent.kind() {
        AgentKind::Grok => crate::agent::grok::make_completion_event(task_id, &info),
        _ => crate::agent::gemini::make_completion_event(task_id, &info),
    };
    store.insert_event(&event)?;
    Ok(info)
}

async fn persist_outputs(
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
    let _ = tokio::fs::write(paths::transcript_path(task_id.as_str()), &buffer).await;
    if let Some(out_path) = output_path {
        if let Some(response) = crate::agent::grok::extract_response(&buffer) {
            let response_filtered: String = response
                .lines()
                .filter(|line| !is_standalone_milestone_line(line))
                .collect::<Vec<_>>()
                .join("\n");
            tokio::fs::write(out_path, &response_filtered).await?;
        } else if let Some(response) = crate::agent::gemini::extract_response(&buffer) {
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
