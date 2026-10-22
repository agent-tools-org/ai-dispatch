// Single workflow helpers for aid merge.
// Deps: parent merge guards, git helpers, Store, and task types.

use super::*;

#[cfg(test)]
pub(super) fn merge_single(
    store: &Store,
    task_id: &str,
    approve: bool,
    check: bool,
    force: bool,
    target: Option<&str>,
) -> Result<()> {
    merge_single_with_output(store, task_id, approve, check, force, target, true)
}

pub(super) fn merge_single_with_output(
    store: &Store,
    task_id: &str,
    approve: bool,
    check: bool,
    force: bool,
    target: Option<&str>,
    print_summary: bool,
) -> Result<()> {
    let task = store
        .get_task(task_id)?
        .ok_or_else(|| anyhow!("Task '{task_id}' not found"))?;
    let outcome = task_outcome(&task);
    validate_merge_outcome(&task, outcome, force)?;
    ensure_task_worktree_is_safe(&task)?;
    let repo_dir = resolve_repo_dir(task.repo_path.as_deref(), task.worktree_path.as_deref());
    if check {
        return check_single(task_id, &task, &repo_dir);
    }

    if !force
        && let Some(wt) = task.worktree_path.as_deref()
        && std::path::Path::new(wt).exists()
    {
        run_verify_in_worktree(wt, task.verify.as_deref());
    }
    if approve {
        match ask_approval(&task)? {
            ApprovalDecision::Merge => {}
            ApprovalDecision::Skip => return Ok(()),
            ApprovalDecision::Retry => {
                aid_info!("[aid] Boss requested retry");
                return Err(anyhow!("Boss requested retry"));
            }
        }
    }
    if let Some(branch) = merge_source_branch(&task) {
        merge_task_branch(store, task_id, &task, &repo_dir, branch, force, target)?;
    } else {
        report_inplace_merge(&repo_dir, force)?;
    }
    if force {
        record_force_merge_warning(store, &task)?;
    }
    crate::task::lifecycle::mark_merged(store, task_id)?;
    if print_summary {
        println!("Marked {task_id} as merged");
    }
    Ok(())
}

fn merge_task_branch(
    store: &Store,
    task_id: &str,
    task: &Task,
    repo_dir: &str,
    branch: &str,
    force: bool,
    target: Option<&str>,
) -> Result<()> {
    let original_status = task.status;
    ensure_branch_drift_confirmed(&task, force)?;
    if !force
        && let Some(wt) = task.worktree_path.as_deref()
        && std::path::Path::new(wt).exists()
    {
        auto_commit_uncommitted(wt, branch);
        sync_cargo_lock_before_merge(&repo_dir, wt, branch);
    }
    let ahead = commits_ahead(&repo_dir, branch);
    if ahead == 0 {
        aid_error!("[aid] Error: branch {branch} has 0 commits ahead — nothing to merge");
        aid_hint!("[aid] The agent may not have committed its changes.");
        if let Some(wt) = task.worktree_path.as_deref()
            && std::path::Path::new(wt).exists()
        {
            aid_info!("[aid] Worktree preserved at {wt} for manual recovery");
        }
        return Err(anyhow!("No commits to merge from {branch}"));
    }
    aid_info!("[aid] Branch {branch} has {ahead} commit(s) ahead");
    if let Some(target_branch) = target {
        checkout_branch(&repo_dir, target_branch)?;
    }
    merge_branch_result(
        store,
        task_id,
        original_status,
        force,
        repo_dir,
        branch,
        ahead,
    )?;

    Ok(())
}

fn merge_branch_result(
    store: &Store,
    task_id: &str,
    original_status: TaskStatus,
    force: bool,
    repo_dir: &str,
    branch: &str,
    ahead: u32,
) -> Result<()> {
    match git_merge_branch(&repo_dir, branch) {
        MergeResult::Merged => {
            aid_info!("[aid] Merged branch {branch} into current branch");
        }
        MergeResult::AlreadyUpToDate => {
            aid_error!(
                "[aid] Error: git merge reported 'Already up to date' despite {ahead} commit(s)"
            );
            aid_warn!("[aid] This may indicate a repo path mismatch. Worktree preserved.");
            return Err(anyhow!("Merge was a no-op — possible repo_path mismatch"));
        }
        MergeResult::Failed(stderr) => {
            aid_warn!("[aid] Warning: git merge {branch} failed:");
            for line in stderr.lines().take(5) {
                aid_warn!("  {}", line);
            }
            aid_hint!("[aid] Manual merge needed: git merge {branch}");
            let preserved_status = if force {
                original_status
            } else {
                TaskStatus::Done
            };
            crate::task::lifecycle::restore_after_merge_failure(store, task_id, preserved_status)?;
            return Err(anyhow!(
                "Merge failed — resolve manually, then re-run aid merge {task_id}"
            ));
        }
        MergeResult::StashRestoreFailed(error) => {
            aid_error!("[aid] Error: {error}");
            return Err(anyhow!(error));
        }
    }
    Ok(())
}

fn report_inplace_merge(repo_dir: &str, force: bool) -> Result<()> {
    if force {
        return Err(anyhow!("Force merge requires a committed worktree branch"));
    }
    let has_changes = Command::new("git")
        .args(["-C", &repo_dir, "status", "--porcelain"])
        .output()
        .ok()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false);
    if has_changes {
        aid_info!("[aid] In-place edit — changes are in your working tree.");
        aid_hint!("[aid] Review: git diff | Revert: git checkout .");
    } else {
        aid_info!("[aid] In-place edit — no uncommitted changes (may already be committed).");
    }
    Ok(())
}
