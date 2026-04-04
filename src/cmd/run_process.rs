// Agent process lifecycle: spawn, watch, completion update, retry branch helpers.
use anyhow::Result;
use chrono::Local;
use std::sync::Arc;
use tokio::process::Command;

use crate::{store::Store, types::*, watcher};
use crate::cmd::run::RunArgs;
use crate::store::TaskCompletionUpdate;

use super::{clean_output_if_jsonl, fill_empty_output_from_log};

const FAST_FAIL_SNAPSHOT_MS: i64 = 5_000;
const STDERR_EXCERPT_LINES: usize = 8;
const STDERR_EXCERPT_CHARS: usize = 400;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FailureContext {
    pub working_dir: Option<String>,
    pub binary_path: String,
    pub worktree_path: Option<String>,
    pub worktree_created: bool,
}

pub(crate) fn capture_failure_context(
    store: &Store,
    task_id: &TaskId,
    cmd: &Command,
) -> FailureContext {
    let std_cmd = cmd.as_std();
    let task = store.get_task(task_id.as_str()).ok().flatten();
    FailureContext {
        working_dir: std_cmd
            .get_current_dir()
            .map(|path| path.display().to_string()),
        binary_path: std_cmd.get_program().to_string_lossy().into_owned(),
        worktree_path: task.as_ref().and_then(|task| task.worktree_path.clone()),
        worktree_created: task
            .as_ref()
            .and_then(|task| task.worktree_path.as_ref())
            .is_some(),
    }
}

pub(crate) fn insert_phase_error_event(
    store: &Store,
    task_id: &TaskId,
    phase: &str,
    error: &str,
    stderr: Option<&str>,
) {
    let mut detail = format!("Failed during {phase}: {error}");
    if let Some(stderr) = stderr.filter(|stderr| !stderr.is_empty()) {
        detail.push_str("\nStderr: ");
        detail.push_str(stderr);
    }
    let _ = store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Error,
        detail,
        metadata: None,
    });
}

pub(crate) fn stderr_excerpt(task_id: &TaskId) -> Option<String> {
    let stderr_path = crate::paths::stderr_path(task_id.as_str());
    let content = std::fs::read_to_string(stderr_path).ok()?;
    let lines: Vec<&str> = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }
    let start = lines.len().saturating_sub(STDERR_EXCERPT_LINES);
    Some(compact_excerpt(&lines[start..].join(" | "), STDERR_EXCERPT_CHARS))
}

pub(crate) fn resolve_failure_exit_code(
    store: &Store,
    task_id: &TaskId,
    exit_code: Option<i32>,
) -> Option<i32> {
    exit_code.or_else(|| {
        store
            .get_events(task_id.as_str())
            .ok()?
            .iter()
            .rev()
            .find_map(|event| {
                event
                    .detail
                    .rsplit_once("exit code ")
                    .and_then(|(_, tail)| tail.split_whitespace().next())
                    .and_then(|code| code.parse::<i32>().ok())
            })
    })
}

pub(crate) fn record_execution_failure(
    store: &Store,
    task_id: &TaskId,
    duration_ms: i64,
    exit_code: Option<i32>,
    context: &FailureContext,
) {
    let reason = match exit_code {
        Some(code) => format!("agent exited with code {code}"),
        None => "agent process failed".to_string(),
    };
    let stderr = stderr_excerpt(task_id);
    insert_phase_error_event(store, task_id, "execution", &reason, stderr.as_deref());
    insert_fast_fail_snapshot_event(store, task_id, duration_ms, exit_code, context);
}

fn insert_fast_fail_snapshot_event(
    store: &Store,
    task_id: &TaskId,
    duration_ms: i64,
    exit_code: Option<i32>,
    context: &FailureContext,
) {
    if duration_ms >= FAST_FAIL_SNAPSHOT_MS || !matches!(exit_code, Some(code) if code != 0) {
        return;
    }
    let detail = format!(
        "Failure context: working directory: {}; agent binary: {}; worktree path: {}; worktree created: {}",
        context
            .working_dir
            .as_deref()
            .unwrap_or("(inherit current process directory)"),
        if context.binary_path.is_empty() {
            "(unknown)"
        } else {
            context.binary_path.as_str()
        },
        context.worktree_path.as_deref().unwrap_or("(none)"),
        context.worktree_created,
    );
    let _ = store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Error,
        detail,
        metadata: None,
    });
}

fn compact_excerpt(text: &str, max_chars: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut excerpt: String = text.chars().take(max_chars).collect();
    excerpt.push_str("...");
    excerpt
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
    let failure_context = capture_failure_context(store.as_ref(), task_id, &cmd);
    #[cfg(unix)]
    cmd.process_group(0);
    crate::cmd::noninteractive_stdio::configure(&mut cmd);
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            let err = anyhow::Error::new(err).context("Failed to spawn agent process");
            let stderr = stderr_excerpt(task_id)
                .or_else(|| Some("unavailable (process did not start)".to_string()));
            insert_phase_error_event(
                store.as_ref(),
                task_id,
                "agent spawn",
                &err.to_string(),
                stderr.as_deref(),
            );
            return Err(err);
        }
    };
    let info = if streaming {
        watcher::watch_streaming(agent, &mut child, task_id, store, log_path, workgroup_id, Some(idle_timeout), None).await
    } else {
        let out = output_path.map(std::path::Path::new);
        watcher::watch_buffered(agent, &mut child, task_id, store, log_path, out, workgroup_id).await
    };
    let info = match info {
        Ok(info) => info,
        Err(err) => {
            let stderr = stderr_excerpt(task_id);
            insert_phase_error_event(
                store.as_ref(),
                task_id,
                "execution",
                &err.to_string(),
                stderr.as_deref(),
            );
            return Err(err);
        }
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
    let exit_code = resolve_failure_exit_code(store.as_ref(), task_id, info.exit_code);
    if info.status == TaskStatus::Failed {
        record_execution_failure(
            store.as_ref(),
            task_id,
            duration_ms,
            exit_code,
            &failure_context,
        );
    }
    let final_model = info.model.as_deref().or(model);
    let cost_usd = info.cost_usd.or_else(|| info.tokens.and_then(|tokens| crate::cost::estimate_cost(tokens, final_model, agent.kind())));
    store.update_task_completion(TaskCompletionUpdate {
        id: task_id.as_str(),
        status: info.status,
        tokens: info.tokens,
        duration_ms,
        model: final_model,
        cost_usd,
        exit_code,
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
