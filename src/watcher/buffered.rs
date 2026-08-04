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
use crate::types::{CompletionInfo, TaskId, TaskStatus};

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
    if info.status == TaskStatus::Done {
        rate_limit::clear_rate_limit(&agent.kind());
    }
    let event = crate::agent::gemini::make_completion_event(task_id, &info);
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
        if let Some(response) = crate::agent::gemini::extract_response(&buffer) {
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
