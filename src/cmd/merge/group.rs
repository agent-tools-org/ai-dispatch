// Group workflow helpers for aid merge.
// Deps: parent merge guards, git helpers, Store, and task types.

use super::*;

pub(super) fn merge_group_with_output(
    store: &Store,
    group_id: &str,
    approve: bool,
    check: bool,
    force: bool,
    target: Option<&str>,
    print_summary: bool,
) -> Result<()> {
    let tasks = store.list_tasks_by_group(group_id)?;
    if tasks.is_empty() {
        return Err(anyhow!("No tasks found in group '{group_id}'"));
    }
    // Refuse before prompting or deriving any group repo state: a poisoned first task
    // would otherwise seed that state from a main checkout.
    if let Some(first) = tasks.first() {
        ensure_task_worktree_is_safe(first)?;
    }
    if check {
        return check_group(group_id, &tasks);
    }
    for task in &tasks {
        let outcome = task_outcome(task);
        if is_merge_candidate(task, outcome) {
            validate_merge_outcome(task, outcome, force)?;
        }
    }
    if !approve_group_merge(group_id, &tasks, approve)? {
        return Ok(());
    }
    let mut merged = 0;
    let mut skipped = Vec::new();
    for task in &tasks {
        let outcome = task_outcome(task);
        if !is_merge_candidate(task, outcome) {
            skipped.push(format!("{} ({})", task.id, task.status.label()));
            continue;
        }
        if merge_group_task(store, task, force, target, &mut skipped)? {
            merged += 1;
        }
    }
    if print_summary {
        println!("Merged {merged} task(s) in group {group_id}");
    }
    if !skipped.is_empty() {
        aid_info!("[aid] Skipped: {}", skipped.join(", "));
    }
    Ok(())
}

fn merge_group_task(
    store: &Store,
    task: &Task,
    force: bool,
    target: Option<&str>,
    skipped: &mut Vec<String>,
) -> Result<bool> {
    ensure_task_worktree_is_safe(task)?;
    let repo_dir = resolve_repo_dir(task.repo_path.as_deref(), task.worktree_path.as_deref());
    if let Some(branch) = merge_source_branch(task) {
        ensure_branch_drift_confirmed(task, force)?;
        if let Some(wt) = task.worktree_path.as_deref()
            && std::path::Path::new(wt).exists()
        {
            auto_commit_uncommitted(wt, branch);
            sync_cargo_lock_before_merge(&repo_dir, wt, branch);
        }
        let ahead = commits_ahead(&repo_dir, branch);
        if ahead == 0 {
            aid_warn!(
                "[aid] Warning: {} — branch {branch} has 0 commits, skipping",
                task.id
            );
            skipped.push(format!("{} (no commits)", task.id));
            return Ok(false);
        }
        if let Some(target_branch) = target {
            checkout_branch(&repo_dir, target_branch)?;
        }
        if !merge_group_branch(&repo_dir, branch, task, ahead, skipped)? {
            return Ok(false);
        }
    } else {
        aid_info!("[aid] {} — no worktree, edits applied in-place", task.id);
    }
    crate::task::lifecycle::mark_merged(store, task.id.as_str())?;
    Ok(true)
}

fn merge_group_branch(
    repo_dir: &str,
    branch: &str,
    task: &Task,
    ahead: u32,
    skipped: &mut Vec<String>,
) -> Result<bool> {
    match git_merge_branch(&repo_dir, branch) {
        MergeResult::Merged => {
            aid_info!("[aid] Merged branch {branch}");
            run_post_merge_verify(&repo_dir, task.verify.as_deref());
        }
        MergeResult::AlreadyUpToDate => {
            aid_warn!(
                "[aid] Warning: {} — merge was no-op despite {ahead} commit(s)",
                task.id
            );
            skipped.push(format!("{} (merge no-op)", task.id));
            return Ok(false);
        }
        MergeResult::Failed(error) => {
            aid_warn!(
                "[aid] Warning: git merge {branch} failed, skipping {}",
                task.id
            );
            for line in error.lines().take(5) {
                aid_warn!("  {}", line);
            }
            skipped.push(format!("{} (merge conflict)", task.id));
            return Ok(false);
        }
        MergeResult::StashRestoreFailed(error) => {
            aid_error!("[aid] Error: {error}");
            return Err(anyhow!(error));
        }
    }
    Ok(true)
}

fn approve_group_merge(group_id: &str, tasks: &[Task], approve: bool) -> Result<bool> {
    if approve {
        match ask_group_approval(group_id, &tasks)? {
            ApprovalDecision::Merge => {}
            ApprovalDecision::Skip => return Ok(false),
            ApprovalDecision::Retry => {
                aid_info!("[aid] Boss requested retry");
                return Err(anyhow!("Boss requested retry"));
            }
        }
    }
    Ok(true)
}
