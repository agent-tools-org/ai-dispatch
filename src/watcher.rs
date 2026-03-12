// Watcher engine: reads agent stdout/stderr and records events to store.
// Supports streaming JSONL (codex/opencode) and buffered JSON (gemini) modes.

use anyhow::Result;
use chrono::Local;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;

use crate::agent::Agent;
use crate::paths;
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
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("No stdout on child process"))?;
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    // Open log file for raw output
    let mut log_file = tokio::fs::File::create(log_path).await?;
    let mut info = CompletionInfo {
        tokens: None,
        status: TaskStatus::Done,
        model: None,
        cost_usd: None,
    };
    let mut event_count = 0u32;

    // Spawn stderr capture in background
    let stderr_handle = spawn_stderr_capture(child, task_id);

    while let Some(line) = lines.next_line().await? {
        // Write raw line to log
        use tokio::io::AsyncWriteExt;
        log_file.write_all(line.as_bytes()).await?;
        log_file.write_all(b"\n").await?;

        // Parse event
        if let Some(event) = agent.parse_event(task_id, &line) {
            apply_completion_event(&mut info, &event);
            store.insert_event(&event)?;
            event_count += 1;
        }
    }

    // Wait for stderr to finish
    if let Some(handle) = stderr_handle {
        let _ = handle.await;
    }

    // Wait for process exit
    let exit_status = child.wait().await?;
    let status = if exit_status.success() {
        TaskStatus::Done
    } else {
        TaskStatus::Failed
    };

    // Record final event (include stderr hint on failure)
    let stderr_note = if status == TaskStatus::Failed {
        let stderr_path = paths::stderr_path(task_id.as_str());
        if stderr_path.exists() {
            format!(" — stderr: {}", stderr_path.display())
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let detail = format!(
        "{} — {} events, exit code {}{}",
        status.label(),
        event_count,
        exit_status.code().unwrap_or(-1),
        stderr_note,
    );
    store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: if status == TaskStatus::Done {
            EventKind::Completion
        } else {
            EventKind::Error
        },
        detail,
        metadata: None,
    })?;

    info.status = status;
    Ok(info)
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
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("No stdout on child process"))?;
    let mut reader = BufReader::new(stdout);
    let mut buffer = String::new();

    // Spawn stderr capture in background
    let stderr_handle = spawn_stderr_capture(child, task_id);

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

    // Wait for stderr to finish
    if let Some(handle) = stderr_handle {
        let _ = handle.await;
    }

    // Wait for process exit
    let exit_status = child.wait().await?;

    let info = if exit_status.success() {
        agent.parse_completion(&buffer)
    } else {
        CompletionInfo {
            tokens: None,
            status: TaskStatus::Failed,
            model: None,
            cost_usd: None,
        }
    };

    // Record completion event
    let event = crate::agent::gemini::make_completion_event(task_id, &info);
    store.insert_event(&event)?;

    Ok(info)
}

/// Spawn a background task to capture stderr to a file
fn spawn_stderr_capture(
    child: &mut Child,
    task_id: &TaskId,
) -> Option<tokio::task::JoinHandle<()>> {
    let stderr = child.stderr.take()?;
    let stderr_path = paths::stderr_path(task_id.as_str());
    Some(tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        let mut collected = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            collected.push_str(&line);
            collected.push('\n');
        }
        if !collected.is_empty() {
            let _ = tokio::fs::write(&stderr_path, &collected).await;
        }
    }))
}

fn apply_completion_event(info: &mut CompletionInfo, event: &TaskEvent) {
    if event.event_kind != EventKind::Completion {
        return;
    }
    let Some(metadata) = event.metadata.as_ref() else {
        return;
    };

    if let Some(tokens) = metadata.get("tokens").and_then(|value| value.as_i64()) {
        info.tokens = Some(tokens);
    }
    if let Some(model) = metadata.get("model").and_then(|value| value.as_str()) {
        info.model = Some(model.to_string());
    }
    if let Some(cost_usd) = metadata.get("cost_usd").and_then(|value| value.as_f64()) {
        info.cost_usd = Some(cost_usd);
    }
}

#[cfg(test)]
mod tests {
    use super::apply_completion_event;
    use crate::types::{CompletionInfo, EventKind, TaskEvent, TaskId, TaskStatus};
    use chrono::Local;
    use serde_json::json;

    #[test]
    fn completion_metadata_updates_summary_fields() {
        let mut info = CompletionInfo {
            tokens: None,
            status: TaskStatus::Done,
            model: None,
            cost_usd: None,
        };
        let event = TaskEvent {
            task_id: TaskId("t-usage".to_string()),
            timestamp: Local::now(),
            event_kind: EventKind::Completion,
            detail: "completed".to_string(),
            metadata: Some(json!({
                "tokens": 12345,
                "model": "gpt-4.1",
                "cost_usd": 0.12
            })),
        };

        apply_completion_event(&mut info, &event);

        assert_eq!(info.tokens, Some(12345));
        assert_eq!(info.model.as_deref(), Some("gpt-4.1"));
        assert_eq!(info.cost_usd, Some(0.12));
    }

    #[test]
    fn non_completion_events_do_not_change_summary_fields() {
        let mut info = CompletionInfo {
            tokens: Some(10),
            status: TaskStatus::Done,
            model: Some("gpt-4.1".to_string()),
            cost_usd: Some(0.01),
        };
        let event = TaskEvent {
            task_id: TaskId("t-ignore".to_string()),
            timestamp: Local::now(),
            event_kind: EventKind::Reasoning,
            detail: "thinking".to_string(),
            metadata: Some(json!({ "tokens": 999 })),
        };

        apply_completion_event(&mut info, &event);

        assert_eq!(info.tokens, Some(10));
        assert_eq!(info.model.as_deref(), Some("gpt-4.1"));
        assert_eq!(info.cost_usd, Some(0.01));
    }
}
