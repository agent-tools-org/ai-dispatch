// Post-run lifecycle helpers for `aid run`.
// Exports: post_run_lifecycle() and extracted quota/worktree helper functions.
// Deps: run.rs wrappers, hooks, retry/judge flow, store, and task types.
use anyhow::Result;
use std::{path::Path, sync::Arc};
use crate::{hooks, rate_limit, store::Store, types::*};
use crate::cmd::{checklist_scan, judge, retry_logic, show};
#[path = "lifecycle/final_state.rs"]
mod final_state;
pub(crate) use final_state::capture_final_worktree_state;
#[path = "lifecycle/missing_report.rs"]
mod missing_report;
#[path = "quota_continuation.rs"]
mod quota_continuation;
pub(crate) use missing_report::record_missing_report;
#[path = "lifecycle/phases.rs"]
mod phases;
#[path = "lifecycle/steps.rs"]
mod steps;
use phases::run_lifecycle_phases;
use steps::*;
use super::run_dirty::{DirtyWorktreeAction, post_agent_dirty_worktree_cleanup};
use super::run_model_selfheal::maybe_auto_retry_after_model_unavailable;
use super::run_delivery_recovery::maybe_auto_recover_missing_delivery;
use super::run_post::{
    auto_save_task_output, maybe_auto_retry_after_hang, maybe_flag_empty_worktree_diff,
    maybe_run_post_done_audit, read_quota_error_message, rescue_quota_failed_task,
    take_next_cascade_agent, worktree_is_empty_diff_with_base,
};
use super::{RunArgs, inherit_retry_base_branch, iterate_config, maybe_auto_retry_after_checklist_miss, maybe_auto_retry_after_verify_failure, maybe_iterate, maybe_judge_retry, maybe_verify, run, run_agent, run_prompt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LifecycleMode {
    Foreground,
    Background,
}

impl LifecycleMode {
    fn is_foreground(self) -> bool {
        matches!(self, Self::Foreground)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LifecyclePhaseDecision {
    Continue,
    Retry(TaskId),
    Stop,
}

impl From<DirtyWorktreeAction> for LifecyclePhaseDecision {
    fn from(action: DirtyWorktreeAction) -> Self {
        match action {
            DirtyWorktreeAction::Continue => Self::Continue,
            DirtyWorktreeAction::Retry(task_id) => Self::Retry(task_id),
            DirtyWorktreeAction::Failed => Self::Stop,
        }
    }
}

/// Runs every post-run phase, then backs the task up exactly once at its
/// settled state (final status, verify status and result file persisted).
pub(crate) async fn post_run_lifecycle(
    mode: LifecycleMode,
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
    agent_kind: AgentKind,
    agent_display_name: &str,
    effective_dir: Option<&String>,
    repo_path: Option<&String>,
    wt_path: Option<&String>,
    container_name: Option<&str>,
    runtime_hooks: &[hooks::Hook],
    prompt_bundle: &run_prompt::PromptBundle,
    pre_verify_status: TaskStatus,
    pre_task_dirty_paths: Option<&[String]>,
) -> Result<Option<TaskId>> {
    let outcome = run_lifecycle_phases(
        mode, store, task_id, args, agent_kind, agent_display_name, effective_dir, repo_path,
        wt_path, container_name, runtime_hooks, prompt_bundle, pre_verify_status,
        pre_task_dirty_paths,
    )
    .await;
    crate::backup::on_settled(store.as_ref(), task_id.as_str());
    outcome
}

pub(crate) fn foreground_status_hint(store: &Store, task_id: &str) -> Result<Option<String>> {
    let Some(task) = store.get_task(task_id)? else {
        return Ok(None);
    };
    if let Some(hint) = merge_hint_for_task(&task) {
        return Ok(Some(hint));
    }
    if !matches!(task_outcome(&task), TaskOutcome::Failed) {
        return Ok(None);
    }
    let next = format!("[aid] Next: aid show {task_id} | aid retry {task_id} -f \"feedback\"");
    let hint = if task.duration_ms.unwrap_or(i64::MAX) < 5000 {
        let stderr = retry_logic::read_stderr_tail(task_id, 3);
        format!("{next}\n[aid] Hint: task failed in <5s — check agent binary is installed and --dir points to a valid repo\n[aid] stderr: {stderr}")
    } else {
        next
    };
    Ok(Some(hint))
}

pub(crate) fn merge_hint_for_task(task: &Task) -> Option<String> {
    if task.status != TaskStatus::Done {
        return None;
    }
    task_outcome(task)
        .is_success()
        .then(|| format!("[aid] Next: aid show {} --diff | aid merge {}", task.id, task.id))
}

fn task_outcome(task: &Task) -> TaskOutcome {
    task.outcome()
}

pub(crate) fn inherit_cascade_target(cascade_args: &mut RunArgs, task: &Task) -> Result<()> {
    if task.repo_path.is_none() && task.worktree_path.is_none() && task.worktree_branch.is_none() {
        cascade_args.existing_task_id = None;
        return Ok(());
    }
    super::apply_retry_target(task, cascade_args)?;
    cascade_args.existing_task_id = None;
    Ok(())
}

pub(crate) fn maybe_flag_hollow_output(
    store: &Store,
    task_id: &TaskId,
    task: &Task,
    base_branch: Option<&str>,
) {
    // Hollow output records factual absence only; content length and style are
    // not evidence about whether the original task contract was satisfied.
    if task.status != TaskStatus::Done {
        return;
    }
    if output_has_content(task) {
        return;
    }
    let no_worktree_changes = match task.worktree_path.as_deref() {
        Some(path) => worktree_is_empty_diff_with_base(Path::new(path), base_branch) == Some(true),
        None => true,
    };
    if !no_worktree_changes {
        return;
    }
    aid_warn!("[aid] Warning: agent completed with no output or worktree changes");
    if let Err(err) = store.update_delivery_assessment(
        task_id.as_str(),
        Some(DeliveryAssessment::HollowOutput),
    ) {
        aid_error!("[aid] Failed to record hollow output delivery assessment: {err}");
    }
    let _ = store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: EventKind::Milestone,
        detail: "Hollow output: no output or worktree changes were observed".to_string(),
        metadata: None,
    });
}
pub(super) fn output_has_content(task: &Task) -> bool {
    if let Some(ref path) = task.output_path {
        if let Ok(content) = std::fs::read_to_string(path) {
            return !content.trim().is_empty();
        }
    }
    let auto_path = crate::paths::task_dir(task.id.as_str()).join("output.md");
    if let Ok(content) = std::fs::read_to_string(auto_path) {
        return !content.trim().is_empty();
    }
    let transcript = crate::paths::transcript_path(task.id.as_str());
    if let Some(content) = super::run_prompt::extract_output_fallback_from_path(&transcript, Some(task.agent_display_name())) {
        return !content.trim().is_empty();
    }
    let log_path = task
        .log_path
        .as_deref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::paths::log_path(task.id.as_str()));
    super::run_prompt::extract_output_fallback_from_path(&log_path, Some(task.agent_display_name()))
        .is_some_and(|content| !content.trim().is_empty())
}
