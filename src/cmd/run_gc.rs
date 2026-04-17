// Auto-GC hooks for successful task and workgroup completion.
// Exports maybe_auto_gc_after_completion() for post-run lifecycle integration.
// Deps: crate::project, crate::store::Store, crate::types::Task, crate::worktree_gc.

use crate::cmd::run::RunArgs;
use crate::project;
use crate::store::Store;
use crate::types::{Task, TaskId, TaskStatus};
use crate::worktree_gc::{
    BranchDeleteOutcome, MergeReason, WorktreeRemoveOutcome, branch_merge_reason,
    delete_local_branch, detect_default_branch, is_managed_branch, managed_branch_prefixes,
    remove_worktree_path,
};
use anyhow::Result;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(crate) fn maybe_auto_gc_after_completion(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
    repo_path_hint: Option<&str>,
) -> Result<()> {
    if !args.auto_gc {
        return Ok(());
    }
    let Some(task) = store.get_task(task_id.as_str())? else {
        return Ok(());
    };
    if task.status != TaskStatus::Done {
        return Ok(());
    }

    let project = project::detect_project();
    let prefixes = managed_branch_prefixes(project.as_ref());
    let mut cleaned = BTreeSet::new();

    if args.worktree.is_some() {
        cleanup_task_branch(&task, repo_path_hint, &prefixes)?;
        cleaned.insert(task.id.to_string());
    }

    let Some(group_id) = task.workgroup_id.as_deref() else {
        return Ok(());
    };
    if !group_is_settled(store, group_id)? {
        return Ok(());
    }
    for group_task in store.list_tasks_by_group(group_id)? {
        if group_task.status != TaskStatus::Done || cleaned.contains(group_task.id.as_str()) {
            continue;
        }
        cleanup_task_branch(&group_task, repo_path_hint, &prefixes)?;
    }
    Ok(())
}

fn cleanup_task_branch(task: &Task, repo_path_hint: Option<&str>, prefixes: &[String]) -> Result<()> {
    let Some(branch) = task.worktree_branch.as_deref() else {
        return Ok(());
    };
    let Some(worktree_path) = task.worktree_path.as_deref() else {
        return Ok(());
    };
    if !is_managed_branch(branch, prefixes) {
        return Ok(());
    }

    let repo_dir = repo_dir_for_task(task, repo_path_hint);
    let base_branch = detect_default_branch(&repo_dir)?;
    let Some(reason) = branch_merge_reason(&repo_dir, &base_branch, branch)? else {
        aid_info!("[aid] task branch kept: not yet merged into {base_branch}");
        return Ok(());
    };

    let worktree_path = Path::new(worktree_path);
    match remove_worktree_path(&repo_dir, worktree_path) {
        Ok(WorktreeRemoveOutcome::Removed) => {}
        Ok(WorktreeRemoveOutcome::Missing) => {}
        Err(err) => {
            aid_warn!("[aid] auto-gc skipped for {branch}: {err}");
            return Ok(());
        }
    }

    match delete_local_branch(&repo_dir, branch)? {
        BranchDeleteOutcome::Deleted => {
            aid_info!("[aid] auto-gc removed {branch} ({})", reason.label());
        }
        BranchDeleteOutcome::Missing => {
            aid_info!("[aid] auto-gc removed worktree for {branch} ({})", reason_label(reason));
        }
        BranchDeleteOutcome::Kept(note) => {
            aid_info!("[aid] auto-gc kept branch {branch}: {note}");
        }
    }
    Ok(())
}

fn group_is_settled(store: &Arc<Store>, group_id: &str) -> Result<bool> {
    Ok(store
        .list_tasks_by_group(group_id)?
        .into_iter()
        .all(|task| task.status.is_terminal()))
}

fn repo_dir_for_task(task: &Task, repo_path_hint: Option<&str>) -> PathBuf {
    task.repo_path
        .as_deref()
        .or(repo_path_hint)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn reason_label(reason: MergeReason) -> &'static str {
    reason.label()
}

#[cfg(test)]
#[path = "run_gc_tests.rs"]
mod tests;
