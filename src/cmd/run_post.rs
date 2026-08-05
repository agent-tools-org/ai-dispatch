// Post-lifecycle helpers extracted from `run_lifecycle`.
// Exports hang retry, quota rescue, output persistence, and diff helpers.
// Deps: run retry flow, rate-limit parsing, store, and task types.

use anyhow::Result;
use std::{path::{Path, PathBuf}, sync::Arc};

use crate::{process_monitor, rate_limit, store::Store, types::*};
use crate::cmd::{retry_logic, run_hung_recovery};

use super::{RunArgs, inherit_retry_base_branch, run};

pub(crate) async fn maybe_auto_retry_after_hang(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
) -> Result<Option<TaskId>> {
    let Some(task) = store.get_task(task_id.as_str())? else {
        return Ok(None);
    };
    if task.status != TaskStatus::Failed {
        return Ok(None);
    }

    let events = store.get_events(task_id.as_str())?;
    let Some(context) = process_monitor::hung_context(&events) else {
        return Ok(None);
    };
    let retry_count = prior_hung_retry_count(store.as_ref(), &task)?;
    let Some(retries_left) = hung_retry_retries_left(args, &context, retry_count) else {
        return Ok(None);
    };
    let hung_task = run_hung_recovery::with_hung_context(&task, &context);
    if !run_hung_recovery::should_auto_retry_hung(&task, &context, retry_count) {
        return Ok(None);
    }

    aid_warn!(
        "[aid] Agent hung, auto-retrying ({} retries left)",
        retries_left
    );

    let feedback =
        run_hung_recovery::build_hung_retry_feedback(&hung_task, context.hung_duration_secs);
    let root_prompt = retry_logic::root_prompt(store.as_ref(), &task)
        .unwrap_or_else(|| args.prompt.clone());
    let retry_args = build_hung_retry_args(args, &task, &context, &feedback, &root_prompt)?;

    process_monitor::insert_hung_retry_event(store.as_ref(), task_id)?;
    let retry_id = Box::pin(run(store.clone(), retry_args)).await?;
    Ok(Some(retry_id))
}

fn hung_retry_retries_left(
    args: &RunArgs,
    context: &process_monitor::HungContext,
    retry_count: u32,
) -> Option<u32> {
    if context.transient {
        return Some(
            run_hung_recovery::MAX_TRANSIENT_HUNG_RETRIES
                .saturating_sub(retry_count)
                .saturating_sub(1),
        );
    }
    if args.retry == 0 {
        return None;
    }
    Some(args.retry.saturating_sub(1))
}

fn build_hung_retry_args(
    args: &RunArgs,
    task: &Task,
    context: &process_monitor::HungContext,
    feedback: &str,
    root_prompt: &str,
) -> Result<RunArgs> {
    let mut retry_args = args.clone();
    retry_args.prompt =
        format!("[Previous attempt feedback]\n{feedback}\n\n[Original task]\n{root_prompt}");
    retry_args.retry = if context.transient {
        args.retry
    } else {
        args.retry.saturating_sub(1)
    };
    retry_args.parent_task_id = Some(task.id.as_str().to_string());
    retry_args.repo = task.repo_path.clone().or_else(|| retry_args.repo.clone());
    retry_args.output = task.output_path.clone().or_else(|| retry_args.output.clone());
    retry_args.model = task.requested_model.clone().or_else(|| retry_args.model.clone());
    retry_args.verify = task.verify.clone();
    retry_args.read_only = task.read_only;
    retry_args.budget = task.budget;
    retry_args.background = false;
    super::apply_retry_target(task, &mut retry_args)?;
    inherit_retry_base_branch(args.dir.as_deref(), task, &mut retry_args);
    if context.transient {
        retry_args.session_id = None;
        if let Some((next_agent, remaining_cascade)) = take_next_cascade_agent(args) {
            retry_args.agent_name = next_agent;
            retry_args.cascade = remaining_cascade;
        }
    } else if task.agent.supports_session_resume() {
        retry_args.session_id = task.agent_session_id.clone();
    }
    Ok(retry_args)
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

pub(crate) fn maybe_flag_empty_worktree_diff(
    store: &Store,
    task_id: &TaskId,
    task: &Task,
    base_branch: Option<&str>,
) {
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
    if let Some(true) = worktree_is_empty_diff_with_base(path, base_branch) {
        aid_warn!("[aid] Warning: agent completed but made no code changes in worktree");
        if let Err(err) = store.update_delivery_assessment(
            task_id.as_str(),
            Some(DeliveryAssessment::EmptyDiff),
        ) {
            aid_error!("[aid] Failed to record empty diff delivery assessment: {err}");
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
        .find_map(|path| super::run_prompt::extract_output_fallback_from_path(&path))
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
    worktree_is_empty_diff_with_base(worktree_dir, None)
}

pub(crate) fn worktree_is_empty_diff_with_base(
    worktree_dir: &Path,
    base_branch: Option<&str>,
) -> Option<bool> {
    crate::worktree::capture_worktree_snapshot_with_base(worktree_dir, base_branch)
        .ok()
        .and_then(|snapshot| snapshot.empty_diff)
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
        let _ = crate::task_lifecycle::rescue_to_done(store, task_id);
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
        .filter(|events| process_monitor::was_auto_retried_after_hang(events))
        .count() as u32)
}

fn audit_current_dir(effective_dir: Option<&str>, repo_path: Option<&str>) -> Option<PathBuf> {
    effective_dir
        .or(repo_path)
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

fn find_rate_limit_line(content: &str) -> Option<String> {
    content
        .lines()
        .find_map(rate_limit::extract_rate_limit_message)
}

#[cfg(test)]
#[path = "run_post_tests.rs"]
mod tests;
