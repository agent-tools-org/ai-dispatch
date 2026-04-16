// Post-lifecycle helpers extracted from `run_lifecycle`.
// Exports hang retry, quota rescue, output persistence, and diff helpers.
// Deps: run retry flow, rate-limit parsing, store, and task types.

use anyhow::Result;
use std::{path::{Path, PathBuf}, sync::Arc};

use crate::{rate_limit, store::Store, types::*};
use crate::cmd::{retry_logic, run_hung_recovery};

use super::{RunArgs, inherit_retry_base_branch, run};

pub(crate) async fn maybe_auto_retry_after_hang(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
) -> Result<Option<TaskId>> {
    if args.retry == 0 {
        return Ok(None);
    }

    let Some(task) = store.get_task(task_id.as_str())? else {
        return Ok(None);
    };
    if task.status != TaskStatus::Failed {
        return Ok(None);
    }

    let events = store.get_events(task_id.as_str())?;
    let Some(context) = run_hung_recovery::hung_context(&events) else {
        return Ok(None);
    };
    let retry_count = prior_hung_retry_count(store.as_ref(), &task)?;
    let hung_task = run_hung_recovery::with_hung_context(&task, &context);
    if !run_hung_recovery::should_auto_retry_hung(&hung_task, retry_count) {
        return Ok(None);
    }

    aid_warn!(
        "[aid] Agent hung, auto-retrying ({} retries left)",
        args.retry.saturating_sub(1)
    );

    let feedback =
        run_hung_recovery::build_hung_retry_feedback(&hung_task, context.hung_duration_secs);
    let root_prompt = retry_logic::root_prompt(store.as_ref(), &task)
        .unwrap_or_else(|| args.prompt.clone());
    let mut retry_args = args.clone();
    retry_args.prompt =
        format!("[Previous attempt feedback]\n{feedback}\n\n[Original task]\n{root_prompt}");
    retry_args.retry = args.retry.saturating_sub(1);
    retry_args.parent_task_id = Some(task_id.as_str().to_string());
    retry_args.repo = task.repo_path.clone().or_else(|| retry_args.repo.clone());
    retry_args.output = task.output_path.clone().or_else(|| retry_args.output.clone());
    retry_args.model = task.model.clone().or_else(|| retry_args.model.clone());
    retry_args.verify = task.verify.clone();
    retry_args.read_only = task.read_only;
    retry_args.budget = task.budget;
    retry_args.background = false;
    let (dir, worktree) = super::retry_target(&task);
    retry_args.dir = dir.or_else(|| retry_args.dir.clone());
    retry_args.worktree = worktree.or_else(|| retry_args.worktree.clone());
    inherit_retry_base_branch(args.dir.as_deref(), &task, &mut retry_args);
    if task.agent == AgentKind::OpenCode {
        retry_args.session_id = task.agent_session_id.clone();
    }

    let retry_id = Box::pin(run(store.clone(), retry_args)).await?;
    let _ = run_hung_recovery::insert_hung_retry_event(store.as_ref(), task_id);
    Ok(Some(retry_id))
}

pub(crate) fn maybe_run_post_done_audit(
    store: &Store,
    task_id: &TaskId,
    args: &RunArgs,
    effective_dir: Option<&str>,
    repo_path: Option<&str>,
) -> Result<()> {
    if !args.audit {
        return Ok(());
    }
    let Some(task) = store.get_task(task_id.as_str())? else {
        return Ok(());
    };
    if task.status != TaskStatus::Done || task.audit_verdict.is_some() {
        return Ok(());
    }
    if !crate::aic::is_available() {
        aid_warn!("[aid] --audit requested but 'aic' not found on PATH — skipping cross-audit");
        store.update_task_audit(task_id.as_str(), Some("skipped"), None)?;
        store.insert_event(&TaskEvent {
            task_id: task_id.clone(),
            timestamp: chrono::Local::now(),
            event_kind: EventKind::Milestone,
            detail: "audit skipped: aic binary not found".to_string(),
            metadata: None,
        })?;
        return Ok(());
    }

    let audit_dir = audit_current_dir(effective_dir, repo_path);
    let result = crate::aic::run_audit(task_id.as_str(), audit_dir.as_deref());
    store.update_task_audit(
        task_id.as_str(),
        Some(result.verdict.as_str()),
        result.report_path.as_deref(),
    )?;
    store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: EventKind::Milestone,
        detail: format!("Audit complete: {}", result.verdict),
        metadata: None,
    })?;
    Ok(())
}

pub(crate) fn maybe_flag_empty_worktree_diff(store: &Store, task_id: &TaskId, task: &Task) {
    if task.read_only || task.status != TaskStatus::Done || task.verify_status != VerifyStatus::Skipped {
        return;
    }
    let Some(wt_path) = task.worktree_path.as_deref() else {
        return;
    };
    let path = Path::new(wt_path);
    if !path.exists() {
        return;
    }
    if let Some(true) = worktree_is_empty_diff(path) {
        aid_warn!("[aid] Warning: agent completed but made no code changes in worktree");
        if let Err(err) = store.update_verify_status(task_id.as_str(), VerifyStatus::EmptyDiff) {
            aid_error!("[aid] Failed to record empty diff status: {err}");
        }
    }
}

pub(crate) fn auto_save_task_output(store: &Store, task: &Task) -> Result<()> {
    let transcript = crate::paths::transcript_path(task.id.as_str());
    let log_path = task
        .log_path
        .as_deref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::paths::log_path(task.id.as_str()));
    let Some(content) = [transcript, log_path]
        .into_iter()
        .find_map(|path| crate::cmd::show::extract_messages_from_log(&path, true))
        .filter(|content| !content.is_empty())
    else {
        return Ok(());
    };
    let output_dir = crate::paths::task_dir(task.id.as_str());
    std::fs::create_dir_all(&output_dir)?;
    let output_path = output_dir.join("output.md");
    std::fs::write(&output_path, &content)?;
    store.update_output_path(task.id.as_str(), &output_path.display().to_string())
}

pub(crate) fn worktree_is_empty_diff(worktree_dir: &Path) -> Option<bool> {
    let head = git_diff_stat_output(worktree_dir, &["diff", "--stat", "HEAD"])?;
    let staged = git_diff_stat_output(worktree_dir, &["diff", "--cached", "--stat"])?;
    Some(head.trim().is_empty() && staged.trim().is_empty())
}

pub(crate) fn rescue_quota_failed_task(
    store: &Store,
    task_id: &TaskId,
    quota_error_message: Option<&str>,
) {
    if quota_error_message.is_none() {
        return;
    }
    let Ok(Some(task)) = store.get_task(task_id.as_str()) else {
        return;
    };
    if task.status == TaskStatus::Failed && task.verify_status == VerifyStatus::Passed {
        aid_info!("[aid] Rescuing quota-failed task {} — verify passed", task_id);
        let _ = store.update_task_status(task_id.as_str(), TaskStatus::Done);
    }
}

pub(crate) fn read_quota_error_message(task_id: &TaskId) -> Option<String> {
    let stderr_path = crate::paths::stderr_path(task_id.as_str());
    if let Ok(stderr) = std::fs::read_to_string(&stderr_path)
        && let Some(line) = find_rate_limit_line(&stderr)
    {
        return Some(line);
    }
    let log_path = crate::paths::log_path(task_id.as_str());
    if let Ok(log) = std::fs::read_to_string(&log_path)
        && let Some(line) = find_rate_limit_line(&log)
    {
        return Some(line);
    }
    None
}

pub(crate) fn take_next_cascade_agent(args: &RunArgs) -> Option<(String, Vec<String>)> {
    let mut cascade = args.cascade.clone();
    if cascade.is_empty() {
        None
    } else {
        Some((cascade.remove(0), cascade))
    }
}

fn prior_hung_retry_count(store: &Store, task: &Task) -> Result<u32> {
    let chain = store.get_retry_chain(task.id.as_str())?;
    Ok(chain
        .into_iter()
        .filter(|entry| entry.id != task.id)
        .filter_map(|entry| store.get_events(entry.id.as_str()).ok())
        .filter(|events| run_hung_recovery::was_auto_retried_after_hang(events))
        .count() as u32)
}

fn audit_current_dir(effective_dir: Option<&str>, repo_path: Option<&str>) -> Option<PathBuf> {
    effective_dir
        .or(repo_path)
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

fn git_diff_stat_output(dir: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

fn find_rate_limit_line(content: &str) -> Option<String> {
    content
        .lines()
        .find_map(rate_limit::extract_rate_limit_message)
}
