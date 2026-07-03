// Agent process lifecycle helpers for `aid run`.
// Exports run_agent_process, run_agent_process_with_timeout, and streaming/output helpers.
// Depends on run_prompt, watcher, cost estimation, and store/event types.
use anyhow::Result;
use chrono::Local;
use serde_json::Value;
use std::path::Path;
use std::process;
use std::sync::Arc;
use tokio::process::Command;
use crate::store::Store;
use crate::types::TaskId;

use super::run_prompt;
#[path = "run_agent/timeout.rs"]
mod timeout;
pub(crate) use timeout::run_agent_process_with_timeout;

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
    timeout_policy: crate::timeout_policy::TimeoutPolicy,
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
        timeout_policy,
    })
    .await
}

fn spawn_child_with_log(cmd: &mut Command, log_path: &Path) -> Result<tokio::process::Child> {
    crate::cmd::noninteractive_stdio::configure(cmd);
    match cmd.spawn() {
        Ok(child) => Ok(child),
        Err(err) => {
            let error_msg = format!("Failed to spawn agent process: {err}");
            aid_error!("[aid] {error_msg}");
            write_spawn_error_log(log_path, &error_msg);
            Err(err.into())
        }
    }
}

fn write_spawn_error_log(log_path: &Path, message: &str) {
    let event = serde_json::json!({
        "type": "error",
        "source": "spawn",
        "message": message,
        "timestamp": Local::now().to_rfc3339(),
    });
    let _ = std::fs::write(log_path, format!("{event}\n"));
}

fn write_streaming_output(log_path: &Path, out_path: &Path) {
    let Ok(log_content) = std::fs::read_to_string(log_path) else { return };
    let mut messages = Vec::new();
    let mut current_stream = String::new();
    for line in log_content.lines() {
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            if v.get("type").and_then(|t| t.as_str()) == Some("message")
                && v.get("role").and_then(|r| r.as_str()) == Some("assistant")
                && let Some(content) = v.get("content").and_then(|c| c.as_str())
            {
                if v.get("delta").and_then(|d| d.as_bool()) == Some(true) {
                    current_stream.push_str(content);
                } else {
                    if !current_stream.is_empty() && current_stream != content {
                        messages.push(std::mem::take(&mut current_stream));
                    } else {
                        current_stream.clear();
                    }
                    messages.push(content.to_string());
                }
            }
            if v.get("type").and_then(|t| t.as_str()) == Some("item.completed")
                && let Some(item) = v.get("item")
                && item.get("type").and_then(|t| t.as_str()) == Some("agent_message")
                && let Some(text) = item.get("text").and_then(|t| t.as_str())
            {
                if !current_stream.is_empty() && current_stream != text {
                    messages.push(std::mem::take(&mut current_stream));
                } else {
                    current_stream.clear();
                }
                messages.push(text.to_string());
            }
        }
    }
    if !current_stream.is_empty() {
        messages.push(current_stream);
    }
    let substantive: Vec<String> = messages.into_iter().filter(|message| message.len() > 50).collect();
    let start = substantive.len().saturating_sub(5);
    let output = substantive[start..].join("\n\n---\n\n");
    if !output.is_empty()
        && let Err(err) = std::fs::write(out_path, &output)
    {
        aid_error!("[aid] Failed to write output file: {err}");
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
            aid_warn!("[aid] ⚠ Worktree escape detected! Agent modified {} file(s) in main repo:", dirty.len());
            for line in dirty.iter().take(10) {
                aid_warn!("  {line}");
            }
            if dirty.len() > 10 {
                aid_warn!("  ... and {} more", dirty.len() - 10);
            }
            aid_hint!("[aid] Run `git checkout .` to discard, or review with `git diff`");
        }
    }
}

/// Check if the agent's diff contains files outside the declared scope.
/// Scope entries can be file paths or directory prefixes (e.g. "src/agent/").
pub(crate) fn check_scope_violations(store: &Store, task_id: &TaskId, scope: &[String], dir: Option<&str>) {
    let work_dir = dir.unwrap_or(".");
    let output = process::Command::new("git")
        .args(["diff", "--name-only", "HEAD"])
        .current_dir(work_dir)
        .output();
    let Ok(o) = output else { return };
    if !o.status.success() { return }
    let stdout = String::from_utf8_lossy(&o.stdout);
    let changed: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    if changed.is_empty() { return }
    let violations: Vec<&str> = changed.iter().copied().filter(|file| {
        !scope.iter().any(|s| {
            let s = s.trim_end_matches('/');
            *file == s || file.starts_with(&format!("{s}/"))
        })
    }).collect();
    if violations.is_empty() { return }
    aid_warn!(
        "[aid] Scope violation: {} file(s) modified outside scope",
        violations.len()
    );
    for f in violations.iter().take(10) {
        aid_warn!("  {f}");
    }
    if violations.len() > 10 {
        aid_warn!("  ... and {} more", violations.len() - 10);
    }
    aid_warn!("[aid] Declared scope: {}", scope.join(", "));
    let event = crate::types::TaskEvent {
        task_id: task_id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: crate::types::EventKind::Error,
        detail: format!("Scope violation: {} file(s) outside scope", violations.len()),
        metadata: None,
    };
    let _ = store.insert_event(&event);
}

#[cfg(test)]
#[path = "run_agent/tests.rs"]
mod tests;

fn format_duration(ms: i64) -> String {
    let secs = ms / 1000;
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}
