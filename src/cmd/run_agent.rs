// Agent process lifecycle helpers for `aid run`.
// Exports run_agent_process, run_agent_process_with_timeout, and streaming/output helpers.
// Depends on run_prompt, watcher, cost estimation, and store/event types.
use anyhow::{Context, Result};
use chrono::Local;
use serde_json::Value;
use std::path::Path;
use std::process;
use std::sync::Arc;
use std::time::Instant;
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use crate::store::Store;
use crate::store::TaskCompletionUpdate;
use crate::types::{CompletionInfo, EventKind, TaskEvent, TaskId, TaskStatus};
use crate::watcher;
const DEFAULT_FOREGROUND_TIMEOUT_MINS: u64 = 30;

use super::run_prompt;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_agent_process(
    agent: &dyn crate::agent::Agent,
    cmd: Command,
    task_id: &TaskId,
    store: &Arc<Store>,
    log_path: &Path,
    output_path: Option<&str>,
    model: Option<&str>,
    streaming: bool,
    workgroup_id: Option<&str>,
) -> Result<()> {
    run_prompt::run_agent_process_impl(run_prompt::RunProcessArgs {
        agent,
        cmd,
        task_id,
        store,
        log_path,
        output_path,
        model,
        streaming,
        workgroup_id,
    })
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_agent_process_with_timeout(
    agent: &dyn crate::agent::Agent,
    mut cmd: Command,
    task_id: &TaskId,
    store: &Arc<Store>,
    log_path: &Path,
    output_path: Option<&str>,
    model: Option<&str>,
    streaming: bool,
    workgroup_id: Option<&str>,
    max_duration_mins: Option<i64>,
) -> Result<()> {
    let timeout_mins = max_duration_mins
        .filter(|m| *m > 0)
        .map(|m| m as u64)
        .unwrap_or(DEFAULT_FOREGROUND_TIMEOUT_MINS);
    let deadline = Duration::from_secs(timeout_mins * 60);
    let start = Instant::now();
    let mut child = cmd.spawn().context("Failed to spawn agent process")?;
    let watch_future = async {
        let info = if streaming {
            watcher::watch_streaming(agent, &mut child, task_id, store, log_path, workgroup_id)
                .await?
        } else {
            let output_path = output_path.map(Path::new);
            watcher::watch_buffered(
                agent,
                &mut child,
                task_id,
                store,
                log_path,
                output_path,
                workgroup_id,
            )
            .await?
        };
        Ok::<CompletionInfo, anyhow::Error>(info)
    };

    match timeout(deadline, watch_future).await {
        Ok(Ok(info)) => {
            if streaming
                && let Some(out_path) = output_path
            {
                write_streaming_output(log_path, Path::new(out_path));
            }
            let duration_ms = start.elapsed().as_millis() as i64;
            let final_model = info.model.as_deref().or(model);
            let cost_usd = info.cost_usd.or_else(|| {
                info.tokens
                    .and_then(|tokens| crate::cost::estimate_cost(tokens, final_model, agent.kind()))
            });
            store.update_task_completion(TaskCompletionUpdate {
                id: task_id.as_str(),
                status: info.status,
                tokens: info.tokens,
                duration_ms,
                model: final_model,
                cost_usd,
                exit_code: info.exit_code,
            })?;
            let duration_str = format_duration(duration_ms);
            let tokens_str = info
                .tokens
                .map(|t| format!(", {} tokens", t))
                .unwrap_or_default();
            let cost_str = if cost_usd.is_some() {
                format!(", {}", crate::cost::format_cost(cost_usd))
            } else {
                String::new()
            };
            println!(
                "Task {} {} ({}{}{})",
                task_id,
                info.status.label(),
                duration_str,
                tokens_str,
                cost_str
            );
            Ok(())
        }
        Ok(Err(err)) => Err(err),
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            let duration_ms = start.elapsed().as_millis() as i64;
            store.update_task_completion(TaskCompletionUpdate {
                id: task_id.as_str(),
                status: TaskStatus::Failed,
                tokens: None,
                duration_ms,
                model,
                cost_usd: None,
                exit_code: None,
            })?;
            let detail = format!("Task killed: exceeded {timeout_mins}m timeout");
            let event = TaskEvent {
                task_id: task_id.clone(),
                timestamp: Local::now(),
                event_kind: EventKind::Error,
                detail: detail.clone(),
                metadata: None,
            };
            let _ = store.insert_event(&event);
            eprintln!("[aid] {detail}");
            Err(anyhow::anyhow!(detail))
        }
    }
}

fn write_streaming_output(log_path: &Path, out_path: &Path) {
    let Ok(log_content) = std::fs::read_to_string(log_path) else { return };
    let mut last_message = String::new();
    for line in log_content.lines() {
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            if v.get("type").and_then(|t| t.as_str()) == Some("message")
                && v.get("role").and_then(|r| r.as_str()) == Some("assistant")
                && let Some(content) = v.get("content").and_then(|c| c.as_str())
            {
                if v.get("delta").and_then(|d| d.as_bool()) == Some(true) {
                    last_message.push_str(content);
                } else {
                    last_message = content.to_string();
                }
            }
            if v.get("type").and_then(|t| t.as_str()) == Some("item.completed")
                && let Some(item) = v.get("item")
                && item.get("type").and_then(|t| t.as_str()) == Some("agent_message")
                && let Some(text) = item.get("text").and_then(|t| t.as_str())
            {
                last_message = text.to_string();
            }
        }
    }
    if !last_message.is_empty()
        && let Err(err) = std::fs::write(out_path, &last_message)
    {
        eprintln!("[aid] Failed to write output file: {err}");
    }
}

pub(crate) fn check_worktree_escape(repo_dir: Option<&str>) {
    let dir = repo_dir.unwrap_or(".");
    let output = process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .output();
    if let Ok(o) = output {
        let stdout = String::from_utf8_lossy(&o.stdout);
        let dirty: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
        if !dirty.is_empty() {
            eprintln!("[aid] ⚠ Worktree escape detected! Agent modified {} file(s) in main repo:", dirty.len());
            for line in dirty.iter().take(10) {
                eprintln!("  {line}");
            }
            if dirty.len() > 10 {
                eprintln!("  ... and {} more", dirty.len() - 10);
            }
            eprintln!("[aid] Run `git checkout .` to discard, or review with `git diff`");
        }
    }
}

fn format_duration(ms: i64) -> String {
    let secs = ms / 1000;
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}
