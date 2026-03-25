// Watcher engine: reads agent stdout/stderr and records events to store.
// Exports streaming and buffered watchers plus shared watcher state.
mod extract;
mod progress;
mod stream;
#[cfg(test)]
mod tests;
use anyhow::Result;
use chrono::Local;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::time::{timeout, Duration};
use crate::agent::Agent;
use crate::paths;
use crate::rate_limit;
use crate::store::Store;
use crate::types::*;
use extract::extract_milestone_detail;
#[cfg(test)]
use extract::{extract_finding_detail, parse_milestone_event};
use progress::LoopDetector;
pub(crate) use progress::SyntheticMilestoneTracker;
pub(crate) use stream::{handle_streaming_line, handle_streaming_line_with_session, StreamLineContext};
const HUNG_TIMEOUT: Duration = Duration::from_secs(300);

/// Watch a child process, parse output, store events, return completion info
pub async fn watch_streaming(
    agent: &dyn Agent,
    child: &mut Child,
    task_id: &TaskId,
    store: &Arc<Store>,
    log_path: &std::path::Path,
    workgroup_id: Option<&str>,
    idle_timeout: Option<Duration>,
    max_task_cost: Option<f64>,
) -> Result<CompletionInfo> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("No stdout on child process"))?;
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();
    let mut log_file = tokio::fs::File::create(log_path).await?;
    let mut info = CompletionInfo {
        tokens: None,
        status: TaskStatus::Done,
        model: None,
        cost_usd: None,
        exit_code: None,
    };
    let mut event_count = 0u32;
    let mut session_saved = false;
    let mut loop_detector = LoopDetector::new();
    let mut synthetic_tracker = SyntheticMilestoneTracker::new();
    let mut stderr_handle = spawn_stderr_capture(child, task_id);
    let idle_timeout = idle_timeout.unwrap_or(HUNG_TIMEOUT);
    loop {
        let line = match timeout(idle_timeout, lines.next_line()).await {
            Ok(Ok(Some(line))) => line,
            Ok(Ok(None)) => break,
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => {
                let _ = child.kill().await;
                let event = TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: Local::now(),
                    event_kind: EventKind::Error,
                    detail: format!(
                        "Agent hung: no output for {} seconds",
                        idle_timeout.as_secs()
                    ),
                    metadata: None,
                };
                let _ = store.insert_event(&event);
                info.status = TaskStatus::Failed;
                break;
            }
        };

        use tokio::io::AsyncWriteExt;
        if extract_milestone_detail(&line).is_none() && !is_thinking_delta(&line) {
            log_file.write_all(line.as_bytes()).await?;
            log_file.write_all(b"\n").await?;
        }

        if let Some(detail) = handle_streaming_line_with_session(
            StreamLineContext {
                agent,
                task_id,
                store,
                workgroup_id,
                synthetic_tracker: &mut synthetic_tracker,
            },
            &mut info,
            &mut event_count,
            &line,
            &mut session_saved,
        )? {
            if exceeds_cost_ceiling(info.cost_usd, max_task_cost) {
                let current_cost = info.cost_usd.unwrap_or_default();
                let max_cost = max_task_cost.unwrap_or_default();
                let _ = store.insert_event(&TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: Local::now(),
                    event_kind: EventKind::Error,
                    detail: format!(
                        "Task killed: cost ${:.2} exceeded ceiling ${:.2}",
                        current_cost, max_cost
                    ),
                    metadata: None,
                });
                let _ = child.kill().await;
                info.status = TaskStatus::Failed;
                break;
            }
            loop_detector.push(&detail);
            if loop_detector.is_looping() {
                let _ = store.insert_event(&TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: Local::now(),
                    event_kind: EventKind::Error,
                    detail: "Agent appears stuck in a loop — killing process".to_string(),
                    metadata: None,
                });
                let _ = child.kill().await;
                info.status = TaskStatus::Failed;
                if let Some(handle) = stderr_handle.take() {
                    let _ = handle.await;
                }
                return Ok(info);
            }
        }
    }
    if let Some(handle) = stderr_handle.take() {
        let _ = handle.await;
    }
    let exit_status = child.wait().await?;
    let status = if exit_status.success() {
        TaskStatus::Done
    } else {
        TaskStatus::Failed
    };

    if status == TaskStatus::Done && rate_limit::is_rate_limited(&agent.kind()) {
        rate_limit::clear_rate_limit(&agent.kind());
    }
    let stderr_note = failure_stderr_note(status, task_id, agent);
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
    _workgroup_id: Option<&str>,
) -> Result<CompletionInfo> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("No stdout on child process"))?;
    let mut reader = BufReader::new(stdout);
    let mut buffer = String::new();
    let stderr_handle = spawn_stderr_capture(child, task_id);
    use tokio::io::AsyncReadExt;
    reader.read_to_string(&mut buffer).await?;
    let filtered: String = buffer
        .lines()
        .filter(|line| extract_milestone_detail(line).is_none())
        .collect::<Vec<_>>()
        .join("\n");
    tokio::fs::write(log_path, &filtered).await?;

    if let Some(out_path) = output_path {
        if let Some(response) = crate::agent::gemini::extract_response(&buffer) {
            let response_filtered: String = response
                .lines()
                .filter(|line| extract_milestone_detail(line).is_none())
                .collect::<Vec<_>>()
                .join("\n");
            tokio::fs::write(out_path, &response_filtered).await?;
        } else {
            tokio::fs::write(out_path, &filtered).await?;
        }
    }
    if let Some(handle) = stderr_handle {
        let _ = handle.await;
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

fn exceeds_cost_ceiling(current_cost: Option<f64>, max_task_cost: Option<f64>) -> bool {
    matches!(
        (current_cost, max_task_cost),
        (Some(current_cost), Some(max_task_cost)) if current_cost > max_task_cost
    )
}

fn failure_stderr_note(status: TaskStatus, task_id: &TaskId, agent: &dyn Agent) -> String {
    if status != TaskStatus::Failed {
        return String::new();
    }
    let stderr_path = paths::stderr_path(task_id.as_str());
    if !stderr_path.exists() {
        return String::new();
    }
    if let Ok(stderr_content) = std::fs::read_to_string(&stderr_path) {
        for line in stderr_content.lines() {
            if let Some(message) = rate_limit::extract_rate_limit_message(line) {
                rate_limit::mark_rate_limited(&agent.kind(), &message);
                break;
            }
        }
    }
    format!(" — stderr: {}", stderr_path.display())
}

fn is_thinking_delta(line: &str) -> bool {
    line.contains("\"type\":\"thinking\"")
}
