// Agent process lifecycle: spawn, watch, completion update, retry branch helpers.
use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::process::Command;

use crate::{store::Store, types::*, watcher};
use crate::cmd::run::RunArgs;
use crate::store::TaskCompletionUpdate;

use super::{clean_output_if_jsonl, fill_empty_output_from_log};

pub(crate) struct RunProcessArgs<'a> {
    pub agent: &'a dyn crate::agent::Agent,
    pub cmd: Command,
    pub task_id: &'a TaskId,
    pub store: &'a Arc<Store>,
    pub log_path: &'a std::path::Path,
    pub output_path: Option<&'a str>,
    pub model: Option<&'a str>,
    pub streaming: bool,
    pub workgroup_id: Option<&'a str>,
}

/// Clean up any lingering child processes in the process group (Unix only).
#[cfg(unix)]
fn cleanup_process_group(child: &tokio::process::Child) {
    if let Some(pid) = child.id() {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGTERM);
        }
    }
}

pub(crate) async fn run_agent_process_impl(args: RunProcessArgs<'_>) -> Result<()> {
    let RunProcessArgs {
        agent,
        mut cmd,
        task_id,
        store,
        log_path,
        output_path,
        model,
        streaming,
        workgroup_id,
    } = args;
    let start = std::time::Instant::now();
    let idle_timeout = crate::idle_timeout::idle_timeout_from_tokio_command(&cmd);
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd.spawn().context("Failed to spawn agent process")?;
    let info = if streaming {
        watcher::watch_streaming(agent, &mut child, task_id, store, log_path, workgroup_id, Some(idle_timeout), None).await?
    } else {
        let out = output_path.map(std::path::Path::new);
        watcher::watch_buffered(agent, &mut child, task_id, store, log_path, out, workgroup_id).await?
    };
    // SIGTERM orphaned child processes — no sleep needed on normal exit
    #[cfg(unix)]
    cleanup_process_group(&child);
    let _ = child.kill().await;
    let _ = child.wait().await;
    let output_path = output_path.map(std::path::Path::new);
    fill_empty_output_from_log(log_path, output_path)?;
    if let Some(out_path) = output_path {
        clean_output_if_jsonl(out_path)?;
    }
    let duration_ms = start.elapsed().as_millis() as i64;
    let final_model = info.model.as_deref().or(model);
    let cost_usd = info.cost_usd.or_else(|| info.tokens.and_then(|tokens| crate::cost::estimate_cost(tokens, final_model, agent.kind())));
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
    let tokens_str = info.tokens.map(|t| format!(", {} tokens", t)).unwrap_or_default();
    let cost_str = if cost_usd.is_some() { format!(", {}", crate::cost::format_cost(cost_usd)) } else { String::new() };
    let fail_reason = if info.status == TaskStatus::Failed {
        store.latest_error(task_id.as_str())
            .map(|r| format!("\n[aid] Reason: {r}"))
            .unwrap_or_default()
    } else {
        String::new()
    };
    println!("Task {} {} ({}{}{}){}", task_id, info.status.label(), duration_str, tokens_str, cost_str, fail_reason);
    Ok(())
}

pub(crate) fn notify_task_completion(store: &Store, task_id: &TaskId) -> Result<()> {
    if let Some(task) = store.get_task(task_id.as_str())? {
        crate::notify::notify_completion(&task);
    }
    Ok(())
}

/// Get the current branch name of a git repo (None if detached HEAD or error)
pub(crate) fn current_branch(repo_dir: &std::path::Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() { return None; }
    let branch = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if branch == "HEAD" { return None; } // detached HEAD
    Some(branch)
}

pub(crate) fn inherit_retry_base_branch_impl(repo_dir: Option<&str>, task: &Task, retry_args: &mut RunArgs) {
    if retry_args.base_branch.is_some() || retry_args.worktree.is_none() { return; }
    let Some(branch) = task.worktree_branch.as_deref() else { return; };
    if retry_args.worktree.as_deref() == Some(branch) { return; }
    let repo_dir = std::path::Path::new(task.repo_path.as_deref().or(retry_args.repo.as_deref()).or(repo_dir).unwrap_or("."));
    if let Ok(true) = crate::worktree::branch_has_commits_ahead_of_main(repo_dir, branch) { retry_args.base_branch = Some(branch.to_string()); }
}

pub(crate) fn retry_target(task: &Task) -> (Option<String>, Option<String>) {
    match task.worktree_path.as_ref() {
        Some(path) if std::path::Path::new(path).exists() => (Some(path.clone()), None),
        Some(_) => (None, task.worktree_branch.clone()),
        None => (None, None),
    }
}

fn format_duration(ms: i64) -> String {
    let secs = ms / 1000;
    if secs < 60 { format!("{secs}s") } else { format!("{}m {:02}s", secs / 60, secs % 60) }
}
