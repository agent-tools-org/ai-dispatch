// Watcher engine: reads agent stdout/stderr and records events to store.
// Exports streaming and buffered watchers plus shared watcher state.
mod buffered;
mod esc;
mod extract;
mod progress;
mod stderr;
mod stream;
#[cfg(test)]
mod tests;
#[cfg(test)]
#[path = "watcher/transcript_tests.rs"]
mod transcript_tests;
#[cfg(test)]
#[path = "watcher/streaming_tests.rs"]
mod streaming_tests;
#[cfg(test)]
#[path = "watcher/streaming_completion_tests.rs"]
mod streaming_completion_tests;

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
use stderr::{drain_stderr_capture, spawn_stderr_capture};
pub(crate) use progress::SyntheticMilestoneTracker;
pub(crate) use stream::{
    StreamLineContext, apply_codex_delivery_guard, handle_streaming_line_with_session,
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
    let mut saw_completion_event = false;
    let mut synthetic_tracker = SyntheticMilestoneTracker::new();
    let mut delivery_evidence = DeliveryEvidence::default();
    let mut last_event_detail: Option<String> = None;
    let mut full_output = String::new();
    let stderr_handle = spawn_stderr_capture(child, task_id);
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
            full_output.push_str(&line);
            full_output.push('\n');
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
            if event_detail.kind == EventKind::Completion {
                saw_completion_event = true;
            }
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
        }
    }
    if let Some(handle) = stderr_handle {
        drain_stderr_capture(handle).await;
    }
    let exit_status = child.wait().await?;
    let mut status = if exit_status.success() {
        TaskStatus::Done
    } else {
        TaskStatus::Failed
    };
    info.exit_code = exit_status.code();
    let task = store.get_task(task_id.as_str()).ok().flatten();
    let delivery_outcome = delivery_evidence.validate();
    let delivered = if agent.kind() == AgentKind::Codex {
        matches!(&delivery_outcome, DeliveryOutcome::Delivered)
    } else {
        saw_completion_event
    };
    if agent.kind() == AgentKind::Codex {
        status = apply_codex_delivery_guard(
            store,
            task_id,
            status,
            delivery_outcome,
            exit_status.code(),
        );
    }

    if status == TaskStatus::Done {
        info.status = status;
        let parsed = agent.parse_completion(&full_output);
        crate::agent::stream_completion::merge_parsed_completion(&mut info, parsed);
        status = info.status;
    }

    // Checked before the clear below: a quota refusal arrives as exit 0 text.
    // agy and other plain-text CLIs never echo their model, so the group a
    // quota belongs to is only knowable from what aid dispatched.
    let dispatched_model = task.as_ref().and_then(|t| t.requested_model.as_deref());
    let quota = crate::agent::stream_completion::record_quota_exhaustion_with_delivery(
        &full_output,
        agent.kind(),
        agent.rate_limit_name(),
        info.model.as_deref().or(dispatched_model),
        delivered,
    );
    if quota.should_fail() {
        status = TaskStatus::Failed;
        info.status = status;
    }

    // A run that delivered *and* hit a refusal stays Done, so this clear would
    // erase the marker written microseconds earlier and hand routing back a
    // provider that is out.
    if status == TaskStatus::Done && !quota.recorded() {
        let model = info.model.as_deref().or(dispatched_model);
        rate_limit::clear_rate_limit_for_model(&agent.kind(), agent.rate_limit_name(), model);
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
    let Some(metadata) = event.metadata.as_ref() else {
        return;
    };
    // A model announcement is evidence wherever it appears in the stream, not
    // only on a completion event. copilot names its model on a Milestone
    // (`session.tools_updated` -> `parse_model_update`) and droid names it on
    // its very first `system/init` line; neither repeats it at completion.
    // Gating the whole function on `EventKind::Completion` dropped both, which
    // is why copilot recorded 19 of 20 tasks with no model and droid 14 of 15,
    // while `"model":"gpt-5-mini"` sat in copilot's log three times over and
    // `"model":"claude-opus-5"` on line 0 of droid's.
    if let Some(model) = metadata.get("model").and_then(|value| value.as_str()) {
        info.model = Some(model.to_string());
    }
    // Tokens and cost stay completion-only: a mid-run value is partial, and
    // letting an interim total overwrite the final one would understate both.
    if event.event_kind != EventKind::Completion {
        return;
    }
    if let Some(tokens) = metadata.get("tokens").and_then(|value| value.as_i64()) {
        info.tokens = Some(tokens);
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
        rate_limit::mark_rate_limited_for_message(
            &agent.kind(),
            agent.rate_limit_name(),
            &message,
        );
    }
    format!(" — stderr: {}", stderr_path.display())
}

fn is_thinking_delta(line: &str) -> bool {
    line.contains("\"type\":\"thinking\"")
}
