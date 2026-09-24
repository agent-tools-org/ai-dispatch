// Handler for `aid run <agent> <prompt>` — dispatch a task to an AI CLI.
// Orchestrates module wiring, shared wrappers, and workspace symlink handling.
// Depends on run submodules, retry/judge helpers, store, and task types.
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use crate::cmd::{judge, retry_logic};
use crate::store::Store;
use crate::types::*;
#[cfg(test)]
pub(crate) use crate::paths;
#[path = "args.rs"]
mod run_args;
#[path = "validate.rs"]
mod run_validate;
#[path = "prompt.rs"]
mod run_prompt;
#[path = "agent.rs"]
mod run_agent;
#[path = "bestof.rs"]
mod run_bestof;
#[path = "lifecycle.rs"]
mod run_lifecycle;
#[path = "dirty.rs"]
mod run_dirty;
#[path = "iterate.rs"]
mod run_iterate;
#[path = "post.rs"]
mod run_post;
#[path = "model_selfheal.rs"]
mod run_model_selfheal;
#[path = "delivery_recovery.rs"]
mod run_delivery_recovery;
#[path = "dispatch_resolve.rs"]
mod run_dispatch_resolve;
#[path = "dispatch_cost_ceiling.rs"]
mod run_dispatch_cost_ceiling;
#[path = "dispatch_claim.rs"]
mod run_dispatch_claim;
#[path = "dispatch_prepare.rs"]
mod run_dispatch_prepare;
#[path = "dispatch_worktree.rs"]
mod run_dispatch_worktree;
#[path = "task_profile.rs"]
mod run_task_profile;
#[path = "delegation.rs"]
mod run_delegation;
#[path = "dispatch_guard.rs"]
mod run_dispatch_guard;
#[path = "foreground_watch.rs"]
mod run_foreground_watch;
#[path = "dispatch_execute.rs"]
mod run_dispatch_execute;
#[path = "dispatch.rs"]
mod run_dispatch;
pub(crate) use self::run_agent::run_agent_process_with_cost;
pub(crate) use self::run_dispatch::run;
pub(crate) use self::run_iterate::iterate_config;
pub(crate) use self::run_lifecycle::capture_final_worktree_state;
pub(crate) use self::run_lifecycle::{
    foreground_status_hint, post_run_lifecycle, LifecycleMode,
};
pub(crate) use self::run_prompt::PromptBundle;
#[allow(unused_imports)]
pub(crate) use self::run_iterate::IterateConfig;
pub(crate) use self::run_iterate::maybe_iterate;
pub(crate) use self::run_args::{NO_SKILL_SENTINEL, RunArgs};
pub(crate) use self::run_delegation::task_depth;
use self::run_args::{context_file_from_spec, preview_prompt, resolve_max_duration_mins, resolve_prompt_input};
#[cfg(test)]
use self::run_validate::{IdConflict, resolve_id_conflict, validate_dispatch};

pub(crate) struct WorkspaceSymlinkGuard { link_path: Option<PathBuf> }

impl WorkspaceSymlinkGuard {
    pub(crate) fn create(
        agent_kind: AgentKind,
        group_id: Option<&str>,
        effective_dir: Option<&str>,
    ) -> Result<Self> {
        if !agent_kind.sandboxed_fs() {
            return Ok(Self { link_path: None });
        }
        let Some(group_id) = group_id else {
            return Ok(Self { link_path: None });
        };
        let workspace = crate::paths::workspace_dir(group_id)?;
        if !workspace.is_dir() {
            return Ok(Self { link_path: None });
        }
        let link_path = Path::new(effective_dir.unwrap_or(".")).join(".aid-workspace");
        if link_path.exists() {
            return Ok(Self { link_path: None });
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&workspace, &link_path)?;
            return Ok(Self { link_path: Some(link_path) });
        }
        #[cfg(not(unix))]
        {
            let _ = workspace;
            let _ = link_path;
            Ok(Self { link_path: None })
        }
    }
}

impl Drop for WorkspaceSymlinkGuard {
    fn drop(&mut self) {
        if let Some(link_path) = self.link_path.take() {
            let _ = std::fs::remove_file(link_path);
        }
    }
}

#[cfg(test)] mod tests;
#[cfg(test)] #[path = "dry_run_tests.rs"] mod run_dry_run_tests;
#[cfg(test)] mod checklist_tests;
#[cfg(test)] #[path = "lifecycle_tests.rs"] mod run_lifecycle_tests;
#[cfg(test)] #[path = "lifecycle_cleanup_tests.rs"] mod run_lifecycle_cleanup_tests;
#[cfg(test)] #[path = "lifecycle_output_tests.rs"] mod run_lifecycle_output_tests;
#[cfg(test)] #[path = "lifecycle/final_state_tests.rs"] mod run_lifecycle_final_state_tests;
#[cfg(test)] #[path = "lifecycle/missing_report_tests.rs"] mod run_lifecycle_missing_report_tests;
#[cfg(test)] #[path = "lifecycle_verify_gate_tests.rs"] mod run_lifecycle_verify_gate_tests;
#[cfg(test)] #[path = "verify_gate_tests.rs"] mod run_verify_gate_tests;
#[cfg(test)] #[path = "verify_infrastructure_tests.rs"] mod run_verify_infrastructure_tests;
#[cfg(test)] mod cargo_route_regression_tests;
#[cfg(test)] #[path = "cascade_tests.rs"] mod run_cascade_tests;
#[cfg(test)] #[path = "audit_tests.rs"] mod audit;
#[cfg(test)] #[path = "retry_target_tests.rs"] mod run_retry_target_tests;

pub(crate) fn inherit_retry_base_branch(repo_dir: Option<&str>, task: &Task, retry_args: &mut RunArgs) { run_prompt::inherit_retry_base_branch_impl(repo_dir, task, retry_args); }
pub(crate) fn retry_target(task: &Task) -> Result<(Option<String>, Option<String>)> { run_prompt::retry_target(task) }
pub(crate) fn apply_retry_target(task: &Task, retry_args: &mut RunArgs) -> Result<()> { run_prompt::apply_retry_target(task, retry_args) }
#[cfg(test)]
fn take_next_cascade_agent(args: &RunArgs) -> Option<(String, Vec<String>)> { run_post::take_next_cascade_agent(args) }
pub(crate) fn switch_agent(args: &mut RunArgs, next_agent: String) { run_post::switch_agent(args, next_agent) }
#[cfg(test)]
fn auto_save_task_output(store: &Store, task: &Task) -> Result<()> { run_post::auto_save_task_output(store, task) }
pub(crate) fn rescue_quota_failed_task(store: &Store, task_id: &TaskId, quota_error_message: Option<&str>) { run_post::rescue_quota_failed_task(store, task_id, quota_error_message); }
pub(crate) fn read_quota_error_message(task_id: &TaskId, agent: &crate::types::AgentKind) -> Option<String> { run_post::read_quota_error_message(task_id, agent) }
#[cfg(test)]
fn worktree_is_empty_diff(worktree_dir: &Path) -> Option<bool> { run_post::worktree_is_empty_diff(worktree_dir) }
#[cfg(test)]
fn maybe_run_post_done_audit(
    store: &Store,
    task_id: &TaskId,
    args: &RunArgs,
    effective_dir: Option<&str>,
    repo_path: Option<&str>,
) -> Result<()> {
    run_post::maybe_run_post_done_audit(store, task_id, args, effective_dir, repo_path)
}
#[cfg(test)]
fn final_dirty_assertion(
    store: &Store,
    task_id: &TaskId,
    dir: &str,
    read_only: bool,
    baseline: Option<&[String]>,
) -> Result<bool> {
    run_dirty::final_dirty_assertion(store, task_id, dir, read_only, baseline)
}

pub(crate) fn maybe_cleanup_fast_fail(store: &Store, task_id: &TaskId, task: &Task) { run_prompt::maybe_cleanup_fast_fail_impl(store, task_id, task); }
pub(crate) fn maybe_verify(
    store: &Store,
    task_id: &TaskId,
    verify: Option<&str>,
    dir: Option<&str>,
    container_name: Option<&str>,
) {
    run_prompt::maybe_verify_impl(store, task_id, verify, dir, container_name);
}
pub(crate) async fn maybe_auto_retry_after_verify_failure(store: &Arc<Store>, task_id: &TaskId, args: &RunArgs, pre_verify_status: TaskStatus) -> Result<Option<TaskId>> {
    run_prompt::maybe_auto_retry_after_verify_failure_impl(store, task_id, args, pre_verify_status).await
}
pub(crate) async fn maybe_auto_retry_after_checklist_miss(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
    checklist_result: Option<&crate::cmd::checklist_scan::ChecklistResult>,
) -> Result<Option<TaskId>> {
    run_prompt::maybe_auto_retry_after_checklist_miss_impl(store, task_id, args, checklist_result).await
}
pub(crate) async fn maybe_judge_retry(store: &Arc<Store>, args: &RunArgs, task_id: &TaskId) -> Result<Option<TaskId>> {
    if args.judge_retry {
        return Ok(None);
    }
    let judge_agent = match args.judge.as_deref().map(str::trim).filter(|agent| !agent.is_empty()) {
        Some(agent) => agent,
        None => return Ok(None),
    };
    let task = match store.get_task(task_id.as_str())? {
        Some(task) => task,
        None => return Ok(None),
    };
    if task.status != TaskStatus::Done {
        return Ok(None);
    }
    let judge_result = match judge::judge_task(&task, judge_agent, &args.prompt).await {
        Ok(result) => result,
        Err(error) => {
            aid_warn!("[aid] Judge inconclusive: {error}");
            let _ = store.insert_event(&TaskEvent {
                task_id: task_id.clone(),
                timestamp: chrono::Local::now(),
                event_kind: EventKind::Milestone,
                detail: format!("Judge inconclusive: {error}"),
                metadata: Some(serde_json::json!({ "judge_inconclusive": true })),
            });
            return Ok(None);
        }
    };
    if judge_result.passed {
        println!("[aid] Judge approved");
        return Ok(None);
    }
    let feedback = judge_result.feedback.trim();
    aid_info!(
        "[aid] Judge requested retry: {}",
        if feedback.is_empty() { "no feedback provided" } else { feedback }
    );
    let mut retry_args = args.clone();
    let root_prompt = retry_logic::root_prompt(store, &task).unwrap_or_else(|| args.prompt.clone());
    retry_args.prompt = format!(
        "[Judge feedback]\n{}\n\n[Original task]\n{root_prompt}",
        if feedback.is_empty() { "Judge requested retry without feedback" } else { feedback }
    );
    retry_args.judge_retry = true;
    let retry_id = Box::pin(run(store.clone(), retry_args)).await?;
    Ok(Some(retry_id))
}
