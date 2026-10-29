// Post-lifecycle helpers extracted from `run_lifecycle`.
// Exports hang retry, output persistence, quota detection, and diff helpers.
// Deps: run retry flow, rate-limit parsing, store, and task types.

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::{routing::rate_limit, store::Store, types::*};

use super::RunArgs;

#[path = "post_hung.rs"]
mod hung;
pub(crate) use hung::maybe_auto_retry_after_hang;

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
    let preserved_read_only_report = task.read_only && task.status == TaskStatus::Failed
        && crate::support::paths::task_dir(task_id.as_str()).join("result.md").is_file();
    if (task.status != TaskStatus::Done && !preserved_read_only_report) || task.audit_verdict.is_some() {
        return Ok(());
    }
    if !crate::task::aic::is_available() {
        aid_warn!("[aid] --audit requested but 'aic' not found on PATH — skipping cross-audit");
        store.update_task_audit(task_id.as_str(), Some("skipped"), None)?;
        store.insert_event(&TaskEvent::new(
            task_id.clone(),
            EventKind::Milestone,
            "audit skipped: aic binary not found".to_string(),
        ))?;
        return Ok(());
    }

    let audit_dir = audit_current_dir(effective_dir, repo_path);
    let result = crate::task::aic::run_audit(task_id.as_str(), audit_dir.as_deref());
    store.update_task_audit(
        task_id.as_str(),
        Some(result.verdict.as_str()),
        result.report_path.as_deref(),
    )?;
    store.insert_event(&TaskEvent::new(
        task_id.clone(),
        EventKind::Milestone,
        format!("Audit complete: {}", result.verdict),
    ))?;
    Ok(())
}

pub(crate) fn maybe_flag_empty_worktree_diff(
    store: &Store,
    task_id: &TaskId,
    task: &Task,
    base_branch: Option<&str>,
) {
    // Empty delivery is orthogonal to verify: a Passed/TimedOut/Skipped verify
    // can still mean the agent changed nothing. Only read-only tasks skip this
    // warning (no changes are expected). Failed verify already fails the task.
    if task.read_only || task.status != TaskStatus::Done {
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
    let transcript = crate::support::paths::transcript_path(task.id.as_str());
    let log_path = task
        .log_path
        .as_deref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::support::paths::log_path(task.id.as_str()));
    let Some(content) = [transcript, log_path]
        .into_iter()
        .find_map(|path| super::run_prompt::extract_output_fallback_from_path(&path, Some(task.agent_display_name())))
        .filter(|content| !content.is_empty())
    else {
        return Ok(());
    };
    let output_dir = crate::support::paths::task_dir(task.id.as_str());
    std::fs::create_dir_all(&output_dir)?;
    let output_path = output_dir.join("output.md");
    std::fs::write(&output_path, &content)?;
    store.update_output_path(task.id.as_str(), &output_path.display().to_string())
}

#[cfg(test)]
pub(crate) fn worktree_is_empty_diff(worktree_dir: &Path) -> Option<bool> {
    worktree_is_empty_diff_with_base(worktree_dir, None)
}

pub(crate) fn worktree_is_empty_diff_with_base(
    worktree_dir: &Path,
    base_branch: Option<&str>,
) -> Option<bool> {
    crate::workspace::worktree::capture_worktree_snapshot_with_base(worktree_dir, base_branch)
        .ok()
        .and_then(|snapshot| snapshot.empty_diff)
}

pub(crate) fn read_quota_error_message(task_id: &TaskId, agent: &crate::types::AgentKind) -> Option<String> {
    let stderr_path = crate::support::paths::stderr_path(task_id.as_str());
    if let Ok(stderr) = std::fs::read_to_string(&stderr_path)
        && let Some(line) = find_rate_limit_line_stderr(&stderr, agent)
    {
        return Some(line);
    }
    if let Some(log) = crate::routing::quota::channel::read_run_stream(task_id.as_str())
        && let Some(line) = find_rate_limit_line_in_agent_log(&log, agent)
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

pub(super) fn record_failed_quota(task: &Task, agent: AgentKind) -> Option<String> {
    if task.status != TaskStatus::Failed {
        return None;
    }
    let model = task.requested_model.as_deref().or(task.observed_model.as_deref());
    if let Some(message) = read_quota_error_message(&task.id, &agent) {
        rate_limit::mark_rate_limited_for_model(
            &agent, task.custom_agent_name.as_deref(), model, &message,
        );
        return Some(message);
    }
    if agent == AgentKind::Grok {
        return None;
    }
    let log = crate::routing::quota::channel::read_run_stream(task.id.as_str())?;
    let kept = crate::routing::quota::channel::provider_attributable(
        &log, agent, crate::routing::quota::channel::Channel::CliStream,
    );
    let message = crate::agent::stream_completion::quota_line(&kept.unsplit, agent)?;
    rate_limit::mark_untrusted_for_model(
        &agent, task.custom_agent_name.as_deref(), model, &message,
    );
    Some(message)
}

fn audit_current_dir(effective_dir: Option<&str>, repo_path: Option<&str>) -> Option<PathBuf> {
    effective_dir
        .or(repo_path)
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

fn find_rate_limit_line_stderr(content: &str, agent: &crate::types::AgentKind) -> Option<String> {
    rate_limit::refusal_on_channel(content, *agent, crate::routing::quota::channel::Channel::CliStderr)
}

fn find_rate_limit_line_in_agent_log(content: &str, agent: &crate::types::AgentKind) -> Option<String> {
    rate_limit::refusal_on_channel(content, *agent, crate::routing::quota::channel::Channel::CliStream)
}

/// Point `args` at a different agent, dropping any pinned model or session when
/// the agent actually changes.
///
/// A model id means something only inside one CLI. codex's `gpt-5.6-sol`
/// reaching agy is refused outright — agy answers by listing its Gemini models
/// (`t-94d5f8ab`, auto-cascaded from `t-9269aab8` after codex hit its quota).
/// A session id is the same defect one field over: it resumes state inside the
/// CLI that issued it, not a substitute. The cascade exists to escape a failing
/// route; inheriting half of that route defeats it.
///
/// This lives in one place because the rule was previously copied into three
/// call sites and missed at four others — including both primary cascades in
/// `run_lifecycle`, which is how the bug survived the v10.5.1 fix that claimed
/// to cover every switch path. A same-agent retry keeps its model and session:
/// it must still ask for what was asked before, and may resume.
pub(crate) fn switch_agent(args: &mut RunArgs, next_agent: String) {
    if args.agent_name != next_agent {
        args.model = None;
        args.session_id = None;
        args.force_default_model = false;
        args.model_source = crate::agent::model_validation::ModelSource::AidResolved;
    }
    args.agent_name = next_agent;
}
