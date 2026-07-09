// Final worktree state capture for the run lifecycle.
// Exports capture_final_worktree_state for completion, failure, and stop paths.
// Deps: commit helpers, Store, TaskId, and git branch inspection.

use anyhow::Result;
use std::path::Path;

use crate::{commit, store::Store, types::TaskId};

pub(crate) fn capture_final_worktree_state(store: &Store, task_id: &TaskId) -> Result<()> {
    let Some(task) = store.get_task(task_id.as_str())? else {
        return Ok(());
    };
    if task.final_head_sha.is_some() || task.final_branch.is_some() {
        return Ok(());
    }
    let Some(worktree_path) = task.worktree_path.as_deref() else {
        return Ok(());
    };
    if !Path::new(worktree_path).exists() {
        return Ok(());
    }
    let final_head_sha = match commit::head_sha(worktree_path) {
        Ok(sha) => Some(sha),
        Err(err) => {
            aid_warn!("[aid] Warning: failed to capture final HEAD for {task_id}: {err}");
            None
        }
    };
    let final_branch = current_branch(Path::new(worktree_path));
    store.update_task_final_state(
        task_id.as_str(),
        final_head_sha.as_deref(),
        final_branch.as_deref(),
    )
}

fn current_branch(repo_dir: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_dir)
        .args(["branch", "--show-current"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let branch = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if branch.is_empty() { None } else { Some(branch) }
}
