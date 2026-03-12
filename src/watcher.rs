// Watcher engine: reads agent stdout and records events to store.
// Supports streaming JSONL (codex) and buffered JSON (gemini) modes.

use anyhow::Result;
use chrono::Local;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use std::sync::Arc;

use crate::agent::Agent;
use crate::store::Store;
use crate::types::*;

/// Watch a child process, parse output, store events, return completion info
pub async fn watch_streaming(
    agent: &dyn Agent,
    child: &mut Child,
    task_id: &TaskId,
    store: &Arc<Store>,
    log_path: &std::path::Path,
) -> Result<CompletionInfo> {
    let stdout = child.stdout.take()
        .ok_or_else(|| anyhow::anyhow!("No stdout on child process"))?;
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    // Open log file for raw output
    let mut log_file = tokio::fs::File::create(log_path).await?;
    let mut last_tokens: Option<i64> = None;
    let mut event_count = 0u32;

    while let Some(line) = lines.next_line().await? {
        // Write raw line to log
        use tokio::io::AsyncWriteExt;
        log_file.write_all(line.as_bytes()).await?;
        log_file.write_all(b"\n").await?;

        // Parse event
        if let Some(event) = agent.parse_event(task_id, &line) {
            // Track token count from completion events
            if event.event_kind == EventKind::Completion {
                last_tokens = crate::agent::codex::extract_tokens_from_detail(&event.detail);
            }
            store.insert_event(&event)?;
            event_count += 1;
        }
    }

    // Wait for process exit
    let exit_status = child.wait().await?;
    let status = if exit_status.success() {
        TaskStatus::Done
    } else {
        TaskStatus::Failed
    };

    // Record final event
    let detail = format!(
        "{} — {} events, exit code {}",
        status.label(),
        event_count,
        exit_status.code().unwrap_or(-1),
    );
    store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: if status == TaskStatus::Done { EventKind::Completion } else { EventKind::Error },
        detail,
    })?;

    Ok(CompletionInfo { tokens: last_tokens, status })
}

/// Watch a non-streaming agent: buffer all output, parse at end
pub async fn watch_buffered(
    agent: &dyn Agent,
    child: &mut Child,
    task_id: &TaskId,
    store: &Arc<Store>,
    log_path: &std::path::Path,
    output_path: Option<&std::path::Path>,
) -> Result<CompletionInfo> {
    let stdout = child.stdout.take()
        .ok_or_else(|| anyhow::anyhow!("No stdout on child process"))?;
    let mut reader = BufReader::new(stdout);
    let mut buffer = String::new();

    // Read all output
    use tokio::io::AsyncReadExt;
    reader.read_to_string(&mut buffer).await?;

    // Write raw output to log
    tokio::fs::write(log_path, &buffer).await?;

    // Write response to output file if requested
    if let Some(out_path) = output_path {
        if let Some(response) = crate::agent::gemini::extract_response(&buffer) {
            tokio::fs::write(out_path, &response).await?;
        } else {
            tokio::fs::write(out_path, &buffer).await?;
        }
    }

    // Wait for process exit
    let exit_status = child.wait().await?;

    let info = if exit_status.success() {
        agent.parse_completion(&buffer)
    } else {
        CompletionInfo { tokens: None, status: TaskStatus::Failed }
    };

    // Record completion event
    let event = crate::agent::gemini::make_completion_event(task_id, &info);
    store.insert_event(&event)?;

    Ok(info)
}
