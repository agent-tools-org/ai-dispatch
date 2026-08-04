// Watcher engine: reads agent stdout/stderr and records events to store.
// Exports streaming and buffered watchers plus shared watcher state.
mod buffered;
mod esc;
mod extract;
mod loop_kill;
mod progress;
mod stderr;
mod stream;
#[cfg(test)]
mod tests;
#[cfg(test)]
#[path = "watcher/loop_detector_tests.rs"]
mod loop_detector_tests;
#[cfg(test)]
#[path = "watcher/transcript_tests.rs"]
mod transcript_tests;
#[cfg(test)]
#[path = "watcher/streaming_tests.rs"]
mod streaming_tests;

pub(crate) use buffered::watch_buffered;
pub(crate) use esc::strip_terminal_escapes;
use anyhow::Result;
use chrono::Local;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::time::{timeout, Duration};
use crate::agent::Agent;
use crate::delivery_guard::{DeliveryEvidence, DeliveryOutcome};
use crate::paths;
use crate::process_group::force_kill_process_group;
use crate::process_monitor;
use crate::rate_limit;
use crate::store::Store;
use crate::types::*;
use extract::is_standalone_milestone_line;
#[cfg(test)]
use extract::{
    extract_finding_detail, extract_milestone_detail, parse_milestone_event,
};
pub(crate) use loop_kill::loop_kill_detail;
use progress::LoopDetector;
use stderr::{drain_stderr_capture, spawn_stderr_capture};
pub(crate) use progress::SyntheticMilestoneTracker;
pub(crate) use stream::{
    handle_streaming_line_with_session, StreamLineContext,
};
/// Watch a child process, parse output, store events, return completion info
pub async fn watch_streaming(
    agent: &dyn Agent,
    child: &mut Child,
    task_id: &TaskId,
    store: &Arc<Store>,
    log_path: &std::path::Path,
    workgroup_id: Option<&str>,
    idle_timeout: Duration,
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
    let mut delivery_evidence = DeliveryEvidence::default();
    let mut last_event_detail: Option<String> = None;
    let mut stderr_handle = spawn_stderr_capture(child, task_id);
    loop {
        let line = match timeout(idle_timeout, lines.next_line()).await {
            Ok(Ok(Some(line))) => line,
            Ok(Ok(None)) => break,
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => {
                force_kill_process_group(child);
                let _ = child.kill().await;
                let _ = process_monitor::insert_hung_detected_events(
                    store.as_ref(),
                    task_id,
                    idle_timeout.as_secs(),
                    event_count,
                    last_event_detail.as_deref(),
                    false,
                );
                info.status = TaskStatus::Failed;
                break;
            }
        };
        if !line.trim().is_empty() { last_event_detail = Some(line.trim().to_string()); }
        if agent.kind() == AgentKind::Codex {
            delivery_evidence.observe_codex_jsonl(&line);
        }

        use tokio::io::AsyncWriteExt;
        if !is_standalone_milestone_line(&line) && !is_thinking_delta(&line) {
            log_file.write_all(line.as_bytes()).await?;
            log_file.write_all(b"\n").await?;
        }

        if let Some(event_detail) = handle_streaming_line_with_session(
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
            let detail = event_detail.detail;
            last_event_detail = Some(detail.clone());
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
                force_kill_process_group(child);
                let _ = child.kill().await;
                info.status = TaskStatus::Failed;
                break;
            }
            loop_detector.push(&detail, event_detail.kind, event_detail.raw_key.as_deref());
            if loop_detector.is_looping() {
                let _ = store.insert_event(&TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: Local::now(),
                    event_kind: EventKind::Error,
                    detail: loop_kill_detail(task_id),
                    metadata: None,
                });
                force_kill_process_group(child);
                let _ = child.kill().await;
                info.status = TaskStatus::Failed;
                if let Some(handle) = stderr_handle.take() {
                    drain_stderr_capture(handle).await;
                }
                break;
            }
        }
    }
    if let Some(handle) = stderr_handle.take() {
        drain_stderr_capture(handle).await;
    }
    let exit_status = child.wait().await?;
    let mut status = if exit_status.success() {
        TaskStatus::Done
    } else {
        TaskStatus::Failed
    };
    info.exit_code = exit_status.code();
    if status == TaskStatus::Done
        && agent.kind() == AgentKind::Codex
        && let DeliveryOutcome::MissingFinalDelivery {
            last_work_kind,
            last_message_chars,
        } = delivery_evidence.validate()
    {
        status = TaskStatus::Failed;
        let _ = store.update_delivery_assessment(
            task_id.as_str(),
            Some(DeliveryAssessment::MissingFinalDelivery),
        );
        let _ = store.insert_event(&TaskEvent {
            task_id: task_id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Error,
            detail: "Missing final delivery: Codex exited after work without a substantive final message".to_string(),
            metadata: Some(serde_json::json!({
                "delivery_guard": "missing_final_delivery",
                "last_work_kind": last_work_kind,
                "last_message_chars": last_message_chars,
                "exit_code": exit_status.code(),
            })),
        });
    }

    if status == TaskStatus::Done {
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
