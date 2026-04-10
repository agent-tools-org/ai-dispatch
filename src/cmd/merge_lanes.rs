// GitButler lane assembly for `aid merge --group --lanes`.
// Exports: merge_group_lanes; deps: gitbutler, merge_git, store, types.

use anyhow::{Result, anyhow};
use std::path::Path;

use crate::store::Store;
use crate::types::TaskStatus;

use super::merge_git::{auto_commit_uncommitted, commits_ahead, resolve_repo_dir};

pub(super) fn merge_group_lanes(store: &Store, group_id: &str) -> Result<()> {
    let tasks = store.list_tasks_by_group(group_id)?;
    if tasks.is_empty() {
        return Err(anyhow!("No tasks found in group '{group_id}'"));
    }
    let repo_dir = resolve_repo_dir(
        tasks.first().and_then(|task| task.repo_path.as_deref()),
        tasks.first().and_then(|task| task.worktree_path.as_deref()),
    );
    if !crate::gitbutler::but_available() {
        return Err(anyhow!("GitButler CLI not found. Install: https://gitbutler.com"));
    }
    crate::gitbutler::ensure_setup(Path::new(&repo_dir))?;

    let mut applied = 0;
    let mut skipped = 0;
    for task in &tasks {
        if task.status != TaskStatus::Done {
            skipped += 1;
            continue;
        }
        let Some(branch) = task.worktree_branch.as_deref() else {
            aid_warn!("[aid] Warning: {} — no worktree branch, skipping", task.id);
            skipped += 1;
            continue;
        };
        if let Some(wt) = task.worktree_path.as_deref()
            && Path::new(wt).exists()
        {
            auto_commit_uncommitted(wt, branch);
        }
        let ahead = commits_ahead(&repo_dir, branch);
        if ahead == 0 {
            aid_warn!("[aid] Warning: {} — branch {branch} has 0 commits, skipping", task.id);
            skipped += 1;
            continue;
        }
        match crate::gitbutler::apply_branch(Path::new(&repo_dir), branch) {
            Ok(()) => {
                aid_info!("[aid] Applied branch {branch} as GitButler lane");
                store.update_task_status(task.id.as_str(), TaskStatus::Merged)?;
                applied += 1;
            }
            Err(err) => {
                let message = err.to_string();
                let first_line = message.lines().next().unwrap_or("unknown error");
                aid_warn!("[aid] Warning: but apply {branch} failed: {first_line}");
                skipped += 1;
            }
        }
    }

    println!("Applied {applied} lane(s) in group {group_id}. Skipped {skipped}.");
    println!("Review the workspace: but status. Push selectively: but push <branch>.");
    println!("Worktrees preserved. Run aid worktree prune to clean up later.");
    Ok(())
}
