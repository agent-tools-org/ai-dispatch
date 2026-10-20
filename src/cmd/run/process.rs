// Agent process lifecycle: spawn, watch, completion update, retry branch helpers.
use anyhow::Result;
use chrono::Local;
use std::sync::Arc;
use tokio::process::Command;

use crate::{store::Store, types::*};
use crate::cmd::run::RunArgs;
use crate::runtime::process::group::cleanup_process_group;
use crate::store::TaskCompletionUpdate;

use super::{clean_output_if_jsonl, fill_empty_output_from_log};

#[path = "process_watch.rs"]
mod execution;

#[path = "retry_target.rs"]
mod run_retry_target;
pub(crate) use run_retry_target::{apply_retry_target, retry_target};
#[path = "spawn_fail.rs"]
mod run_spawn_fail;
pub(crate) use run_spawn_fail::{fail_task_on_agent_spawn, insert_phase_error_event};
use run_spawn_fail::insert_fast_fail_snapshot_event;

const FAST_FAIL_SNAPSHOT_MS: i64 = 5_000;
const STDERR_EXCERPT_LINES: usize = 8;
const STDERR_EXCERPT_CHARS: usize = 400;

pub(crate) struct RunProcessArgs<'a> {
    pub agent: &'a dyn crate::agent::Agent, pub cmd: Command, pub task_id: &'a TaskId,
    pub store: &'a Arc<Store>, pub log_path: &'a std::path::Path,
    pub output_path: Option<&'a str>, pub model: Option<&'a str>, pub streaming: bool,
    pub workgroup_id: Option<&'a str>,
    pub timeout_policy: crate::config::timeout_policy::TimeoutPolicy,
    pub max_task_cost: Option<f64>,
    pub start: std::time::Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FailureContext {
    pub working_dir: Option<String>,
    pub binary_path: String,
    pub worktree_path: Option<String>,
    pub worktree_created: bool,
}

pub(crate) fn capture_failure_context(store: &Store, task_id: &TaskId, cmd: &Command) -> FailureContext {
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

pub(crate) fn stderr_excerpt(task_id: &TaskId) -> Option<String> {
    let stderr_path = crate::support::paths::stderr_path(task_id.as_str());
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

pub(crate) fn resolve_failure_exit_code(store: &Store, task_id: &TaskId, exit_code: Option<i32>) -> Option<i32> {
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

pub(crate) fn record_execution_failure(store: &Store, task_id: &TaskId, duration_ms: i64, exit_code: Option<i32>, context: &FailureContext) {
    let capacity = store.get_task(task_id.as_str()).ok().flatten()
        .and_then(|task| crate::routing::capacity::read(store, task_id, task.agent).map(|_|
            crate::routing::capacity::terminal_label(store, task_id, task.agent, TaskStatus::Failed)));
    let reason = match (capacity, exit_code) {
        (Some(message), _) => message,
        (_, Some(code)) => format!("agent exited with code {code}"),
        (_, None) => "agent process failed".to_string(),
    };
    let stderr = stderr_excerpt(task_id);
    insert_phase_error_event(store, task_id, "execution", &reason, stderr.as_deref());
    if !reason.starts_with("provider capacity") {
        insert_fast_fail_snapshot_event(store, task_id, duration_ms, exit_code, context);
    }
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

fn agent_log_label(agent: &dyn crate::agent::Agent) -> &str {
    agent.rate_limit_name().unwrap_or_else(|| agent.kind().as_str())
}

pub(crate) async fn run_agent_process_impl(mut args: RunProcessArgs<'_>) -> Result<()> {
    let start = args.start;
    let failure_context = capture_failure_context(args.store.as_ref(), args.task_id, &args.cmd);
    if args.agent.kind() == AgentKind::Codex
        && crate::runtime::codex_app_server::available(args.cmd.as_std())
    {
        if let Some(mut info) = crate::runtime::codex_app_server::run(crate::runtime::codex_app_server::Run {
            agent:args.agent,command:args.cmd.as_std(),task:args.task_id,store:args.store,
            log:args.log_path,output:args.output_path,group:args.workgroup_id,
            policy:args.timeout_policy,max_cost:args.max_task_cost,control:None,start,
        })? {
            return complete_agent(&args,&mut info,start,&failure_context);
        }
        args.cmd = tokio::process::Command::from(crate::runtime::codex_app_server::exec_fallback(
            args.cmd.as_std(), args.store, args.task_id,
        )?);
    }
    let mut child = execution::spawn_agent(&mut args)?;
    let mut info = execution::watch_agent(&args, &mut child, start).await?;
    cleanup_process_group(&child);
    let _ = child.kill().await;
    let _ = child.wait().await;
    complete_agent(&args, &mut info, start, &failure_context)
}

fn complete_agent(
    args: &RunProcessArgs<'_>, info: &mut CompletionInfo,
    start: std::time::Instant, failure_context: &FailureContext,
) -> Result<()> {
    let cost_event = apply_cost_ceiling(args, info)?;
    finish_output(args)?;
    let agent = args.agent;
    let task_id = args.task_id;
    let store = args.store;
    let model = args.model;
    let duration_ms = start.elapsed().as_millis() as i64;
    let exit_code = resolve_failure_exit_code(store.as_ref(), task_id, info.exit_code);
    // Same split as runner::record_completion: the observation stands alone,
    // only the cost estimate may fall back to what was requested.
    let (observed_model, attribution_source) = crate::agent::codex::grade_completion_observation(
        agent, store.as_ref(), task_id, info, model,
    );
    let costing_model = observed_model.as_deref().or(model);
    let cost_usd = crate::cost::completion_cost(store, task_id, info, costing_model, agent.kind())?;
    if !crate::task::lifecycle::update_task_completion(store.as_ref(), TaskCompletionUpdate {
        id: task_id.as_str(),
        status: info.status,
        tokens: info.tokens,
        duration_ms,
        observed_model: observed_model.as_deref(),
        attribution_source,
        cost_usd,
        exit_code,
    })? {
        return Ok(());
    }
    if let Some(event) = cost_event {
        let _ = store.insert_event(&event);
    }
    if info.status == TaskStatus::Failed && exit_code != Some(0) {
        record_execution_failure(
            store.as_ref(),
            task_id,
            duration_ms,
            exit_code,
            failure_context,
        );
    }
    print_completion(agent, store, task_id, info, duration_ms, cost_usd);
    Ok(())
}

fn apply_cost_ceiling(args: &RunProcessArgs<'_>, info: &mut CompletionInfo) -> Result<Option<TaskEvent>> {
    let agent = args.agent;
    let task_id = args.task_id;
    let store = args.store;
    let model = args.model;
    let max_task_cost = args.max_task_cost;
    let costing_model = info.model.as_deref().or(model);
    let cost_usd = crate::cost::completion_cost(store, task_id, info, costing_model, agent.kind())?;
    if cost_usd.is_some_and(|cost| max_task_cost.is_some_and(|max| cost > max)) {
        let current_cost = cost_usd.unwrap_or_default();
        let max_cost = max_task_cost.unwrap_or_default();
        info.status = TaskStatus::Failed;
        return Ok(Some(TaskEvent {
            task_id: task_id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Error,
            detail: format!("Task killed: cost ${current_cost:.2} exceeded ceiling ${max_cost:.2}"),
            metadata: None,
        }));
    }
    Ok(None)
}

fn finish_output(args: &RunProcessArgs<'_>) -> Result<()> {
    let agent = args.agent;
    let log_path = args.log_path;
    let output_path = args.output_path;
    let output_path = output_path.map(std::path::Path::new);
    fill_empty_output_from_log(log_path, output_path, Some(agent_log_label(agent)))?;
    if let Some(out_path) = output_path {
        clean_output_if_jsonl(out_path)?;
    }
    Ok(())
}

fn print_completion(
    agent: &dyn crate::agent::Agent, store: &Store, task_id: &TaskId, info: &CompletionInfo,
    duration_ms: i64, cost_usd: Option<f64>,
) {
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
    let label = crate::routing::capacity::terminal_label(store, task_id, agent.kind(), info.status);
    println!("Task {} {} ({}{}{}){}", task_id, label, duration_str, tokens_str, cost_str, fail_reason);
}

pub(crate) fn notify_task_completion(store: &Store, task_id: &TaskId) -> Result<()> {
    if let Some(task) = store.get_task(task_id.as_str())? {
        crate::task::notify::notify_completion(&task);
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
    if let Ok(true) = crate::workspace::worktree::branch_has_commits_ahead_of_main(repo_dir, branch) { retry_args.base_branch = Some(branch.to_string()); }
}

fn format_duration(ms: i64) -> String {
    let secs = ms / 1000;
    if secs < 60 { format!("{secs}s") } else { format!("{}m {:02}s", secs / 60, secs % 60) }
}

#[cfg(test)]
#[path = "agent/timeout_tests.rs"]
mod timeout_tests;
