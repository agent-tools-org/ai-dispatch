// Handler for `aid merge` — mark done task(s) as merged, optionally by workgroup.
// Exports: run()
// Deps: chrono, crate::store::Store, crate::types

use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent, TaskId, TaskOutcome, TaskStatus, VerifyStatus};
use anyhow::{Result, anyhow};
use chrono::Local;
use std::process::{Command, Stdio};
use std::sync::Arc;
#[path = "final_branch.rs"]
mod final_branch;
use final_branch::*;
#[path = "../merge_git.rs"]
mod merge_git;
use merge_git::*;
#[path = "../merge_lanes.rs"]
mod merge_lanes;

#[path = "single.rs"]
mod single;
use single::*;

#[path = "group.rs"]
mod group;
use group::*;

#[path = "approval.rs"]
mod approval;
use approval::*;

pub fn run(
    store: Arc<Store>,
    task_id: Option<&str>,
    group: Option<&str>,
    approve: bool,
    check: bool,
    force: bool,
    target: Option<&str>,
    lanes: bool,
) -> Result<()> {
    run_with_output(
        store, task_id, group, approve, check, force, target, lanes, true,
    )
}

#[cfg(feature = "web")]
pub(crate) fn run_quiet(
    store: Arc<Store>,
    task_id: Option<&str>,
    group: Option<&str>,
    approve: bool,
    check: bool,
    force: bool,
    target: Option<&str>,
    lanes: bool,
) -> Result<()> {
    run_with_output(
        store, task_id, group, approve, check, force, target, lanes, false,
    )
}

fn run_with_output(
    store: Arc<Store>,
    task_id: Option<&str>,
    group: Option<&str>,
    approve: bool,
    check: bool,
    force: bool,
    target: Option<&str>,
    lanes: bool,
    print_summary: bool,
) -> Result<()> {
    if lanes {
        let Some(group_id) = group else {
            return Err(anyhow!("--lanes requires --group"));
        };
        if check {
            return Err(anyhow!(
                "--lanes does not yet support --check (dry-run); run without --check to apply lanes"
            ));
        }
        if target.is_some() {
            return Err(anyhow!(
                "--lanes cannot be combined with --target; lanes apply to the GitButler workspace \
                of the main repo"
            ));
        }
        return merge_lanes::merge_group_lanes(&store, group_id, force);
    }
    match (task_id, group) {
        (Some(id), _) => {
            merge_single_with_output(&store, id, approve, check, force, target, print_summary)
        }
        (_, Some(group_id)) => merge_group_with_output(
            &store,
            group_id,
            approve,
            check,
            force,
            target,
            print_summary,
        ),
        (None, None) => Err(anyhow!("Provide either a task ID or --group <wg-id>")),
    }
}

fn record_force_merge_warning(store: &Store, task: &Task) -> Result<()> {
    let current_task = store
        .get_task(task.id.as_str())?
        .ok_or_else(|| anyhow!("Task '{}' not found", task.id))?;
    let reason = match current_task.verify_status {
        VerifyStatus::Failed => "verification command failed",
        VerifyStatus::InfrastructureFailure => "verification infrastructure failed",
        VerifyStatus::TimedOut => "verification timed out",
        _ => "agent/tests did not complete successfully",
    };
    let detail = format!(
        "Force-merged task {} from status {} — {}",
        current_task.id,
        current_task.status.label(),
        reason,
    );
    aid_warn!("[aid] Warning: {detail}");
    store.insert_event(&TaskEvent::new(TaskId(task.id.to_string()), EventKind::Error, detail.clone()))?;
    store.update_principal_merge_override(task.id.as_str(), &detail)?;
    Ok(())
}

fn task_outcome(task: &Task) -> TaskOutcome {
    task.outcome()
}

pub(crate) fn validate_merge_outcome(task: &Task, outcome: TaskOutcome, force: bool) -> Result<()> {
    if !matches!(task.status, TaskStatus::Done)
        && !(force && matches!(task.status, TaskStatus::Failed | TaskStatus::Stopped))
    {
        return Err(anyhow!(
            "Task '{}' is {} — only DONE tasks can be marked as merged",
            task.id,
            task.status.label()
        ));
    }
    match outcome {
        TaskOutcome::Verified | TaskOutcome::Delivered => Ok(()),
        TaskOutcome::Unverified(reason) => {
            aid_warn!(
                "[aid] Warning: task '{}' has inconclusive verification ({reason:?})",
                task.id
            );
            if force {
                Ok(())
            } else {
                Err(anyhow!(
                    "Task '{}' has inconclusive verification — use --force to merge",
                    task.id
                ))
            }
        }
        TaskOutcome::Broken => {
            aid_warn!(
                "[aid] Warning: task '{}' has VFAIL status — verify failed before merge",
                task.id
            );
            aid_hint!("[aid] Review carefully: aid show {} --diff", task.id);
            if force {
                Ok(())
            } else {
                Err(anyhow!(
                    "Task '{}' verification failed — use --force to merge",
                    task.id
                ))
            }
        }
        TaskOutcome::Failed | TaskOutcome::Stopped if force => Ok(()),
        _ => Err(anyhow!(
            "Task '{}' is {} — only completed tasks can be marked as merged",
            task.id,
            task.status.label()
        )),
    }
}

pub(super) fn is_merge_candidate(task: &Task, outcome: TaskOutcome) -> bool {
    task.status == TaskStatus::Done && outcome.is_merge_candidate()
}

fn check_single(task_id: &str, task: &Task, repo_dir: &str) -> Result<()> {
    ensure_task_worktree_is_safe(task)?;
    match merge_source_branch(task) {
        Some(branch) => {
            warn_branch_drift(task);
            let result = check_merge(repo_dir, branch);
            print_check_result(task_id, &result);
            if let MergeCheckResult::StashRestoreFailed(error) = result {
                return Err(anyhow!(error));
            }
        }
        None => println!("{task_id}: OK (in-place edit)"),
    }
    Ok(())
}

fn check_group(group_id: &str, tasks: &[Task]) -> Result<()> {
    let mut conflicts = 0;
    for task in tasks {
        ensure_task_worktree_is_safe(task)?;
        let repo_dir = resolve_repo_dir(task.repo_path.as_deref(), task.worktree_path.as_deref());
        match merge_source_branch(task) {
            Some(branch) => {
                warn_branch_drift(task);
                let result = check_merge(&repo_dir, branch);
                if matches!(result, MergeCheckResult::Conflict(_)) {
                    conflicts += 1;
                }
                print_check_result(task.id.as_str(), &result);
                if let MergeCheckResult::StashRestoreFailed(error) = result {
                    return Err(anyhow!(error));
                }
            }
            None => println!("{}: OK (in-place edit)", task.id),
        }
    }
    println!(
        "Checked {} task(s) in group {group_id}; conflicts: {conflicts}",
        tasks.len()
    );
    Ok(())
}

fn print_check_result(task_id: &str, result: &MergeCheckResult) {
    match result {
        MergeCheckResult::Ok(commits) => println!("{task_id}: OK ({commits} commit(s))"),
        MergeCheckResult::Conflict(files) => println!("{task_id}: CONFLICT ({})", files.join(", ")),
        MergeCheckResult::StashRestoreFailed(error) => println!("{task_id}: ERROR ({error})"),
    }
}

pub(super) fn ensure_task_worktree_is_safe(task: &Task) -> Result<()> {
    let Some(wt) = task.worktree_path.as_deref() else {
        return Ok(());
    };
    if !std::path::Path::new(wt).exists() {
        return Ok(());
    }
    crate::workspace::worktree::ensure_consumed_worktree_path_is_isolated(
        task.repo_path.as_deref(),
        wt,
        &format!("recorded worktree path for task {}", task.id),
    )
}

#[cfg(test)]
#[path = "final_branch_tests.rs"]
mod final_branch_tests;
#[cfg(test)]
#[path = "../merge_guard_tests.rs"]
mod merge_guard_tests;
#[cfg(test)]
mod tests;
