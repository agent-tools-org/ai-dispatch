// Completed-task worktree cleanup helpers for automatic post-success pruning.
// Exports one lifecycle hook that honors project config, branch state, and shared locks.
// Deps: crate::project, crate::store, crate::types, super::branch_has_commits_ahead_of_main.

use anyhow::Result;
use std::path::Path;

use crate::store::Store;
use crate::types::{TaskId, TaskStatus};

pub(crate) fn cleanup_completed_worktree(store: &Store, task_id: &TaskId) -> Result<()> {
    let Some(task) = store.get_task(task_id.as_str())? else {
        return Ok(());
    };
    if task.status != TaskStatus::Done || task.read_only {
        return Ok(());
    }
    let Some(wt_path) = task.worktree_path.as_deref() else {
        return Ok(());
    };
    if !Path::new(wt_path).exists() {
        return Ok(());
    }
    if keep_worktrees_after_done(&task) {
        return Ok(());
    }
    if store.has_active_worktree_siblings(wt_path, task_id.as_str())? {
        aid_info!("[aid] Preserving worktree {wt_path} — other active tasks share it");
        return Ok(());
    }
    let Some(branch) = task.worktree_branch.as_deref() else {
        return Ok(());
    };
    let repo_dir = task
        .repo_path
        .as_deref()
        .or(task.worktree_path.as_deref())
        .unwrap_or(".");
    if !super::branch_has_commits_ahead_of_main(Path::new(repo_dir), branch)? {
        return Ok(());
    }
    crate::cmd::merge::remove_worktree(repo_dir, wt_path)?;
    aid_info!("[aid] Removed completed worktree {wt_path}");
    Ok(())
}

fn keep_worktrees_after_done(task: &crate::types::Task) -> bool {
    task.repo_path
        .as_deref()
        .and_then(|path| crate::project::detect_project_in(Path::new(path)))
        .or_else(|| {
            task.worktree_path
                .as_deref()
                .and_then(|path| crate::project::detect_project_in(Path::new(path)))
        })
        .map(|project| project.keep_worktrees_after_done)
        .unwrap_or(false)
}
