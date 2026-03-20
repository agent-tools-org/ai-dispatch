// Handler for `aid merge` — mark done task(s) as merged, optionally by workgroup.
// Exports: run()
// Deps: crate::store::Store, crate::types::TaskStatus

use anyhow::{anyhow, Result};
use std::process::{Command, Stdio};
use std::sync::Arc;

use crate::store::Store;
use crate::types::{Task, TaskStatus, VerifyStatus};

#[path = "merge_git.rs"]
pub(crate) mod merge_git;
use merge_git::*;
pub use merge_git::remove_worktree;

pub fn run(store: Arc<Store>, task_id: Option<&str>, group: Option<&str>, approve: bool, check: bool, target: Option<&str>) -> Result<()> {
    match (task_id, group) {
        (Some(id), _) => merge_single(&store, id, approve, check, target),
        (_, Some(group_id)) => merge_group(&store, group_id, approve, check, target),
        (None, None) => Err(anyhow!("Provide either a task ID or --group <wg-id>")),
    }
}

fn merge_single(store: &Store, task_id: &str, approve: bool, check: bool, target: Option<&str>) -> Result<()> {
    let task = store
        .get_task(task_id)?
        .ok_or_else(|| anyhow!("Task '{task_id}' not found"))?;
    if task.status != TaskStatus::Done {
        return Err(anyhow!(
            "Task '{task_id}' is {} — only DONE tasks can be marked as merged",
            task.status.label()
        ));
    }
    if task.verify_status == VerifyStatus::Failed {
        aid_warn!("[aid] Warning: task '{task_id}' has VFAIL status — verify failed before merge");
        aid_hint!("[aid] Review carefully: aid show {task_id} --diff");
    }
    let repo_dir = resolve_repo_dir(task.repo_path.as_deref(), task.worktree_path.as_deref());
    if check {
        return check_single(task_id, &task, &repo_dir);
    }

    // Pre-merge verification: run verify command in worktree
    if let Some(wt) = task.worktree_path.as_deref()
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
    // Auto cherry-pick worktree branch into current branch
    if let Some(ref branch) = task.worktree_branch {
        // Auto-commit any uncommitted changes before merge
        if let Some(wt) = task.worktree_path.as_deref()
            && std::path::Path::new(wt).exists()
        {
            auto_commit_uncommitted(wt, branch);
            sync_cargo_lock_before_merge(&repo_dir, wt, branch);
        }
        // Pre-check: verify branch has commits to merge
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
        match git_merge_branch(&repo_dir, branch) {
            MergeResult::Merged => {
                aid_info!("[aid] Merged branch {branch} into current branch");
            }
            MergeResult::AlreadyUpToDate => {
                aid_error!("[aid] Error: git merge reported 'Already up to date' despite {ahead} commit(s)");
                aid_warn!("[aid] This may indicate a repo path mismatch. Worktree preserved.");
                return Err(anyhow!("Merge was a no-op — possible repo_path mismatch"));
            }
            MergeResult::Failed(stderr) => {
                aid_warn!("[aid] Warning: git merge {branch} failed:");
                for line in stderr.lines().take(5) {
                    aid_warn!("  {}", line);
                }
                aid_hint!("[aid] Manual merge needed: git merge {branch}");
                // Don't clean up worktree — user needs it for manual merge
                store.update_task_status(task_id, TaskStatus::Done)?;
                return Err(anyhow!("Merge failed — resolve manually, then re-run aid merge {task_id}"));
            }
        }
    } else {
        // In-place edit: check if there are uncommitted changes
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
    }
    store.update_task_status(task_id, TaskStatus::Merged)?;
    println!("Marked {task_id} as merged");
    // Clean up worktree only after successful merge
    if let Some(wt) = task.worktree_path.as_deref()
        && std::path::Path::new(wt).exists()
        && let Err(err) = remove_worktree(&repo_dir, wt) {
            aid_warn!("[aid] Warning: failed to clean up worktree {wt}: {err}");
        }
    Ok(())
}

fn merge_group(store: &Store, group_id: &str, approve: bool, check: bool, target: Option<&str>) -> Result<()> {
    let tasks = store.list_tasks_by_group(group_id)?;
    if tasks.is_empty() {
        return Err(anyhow!("No tasks found in group '{group_id}'"));
    }
    if check {
        return check_group(group_id, &tasks);
    }
    if approve {
        match ask_group_approval(group_id, &tasks)? {
            ApprovalDecision::Merge => {}
            ApprovalDecision::Skip => return Ok(()),
            ApprovalDecision::Retry => {
                aid_info!("[aid] Boss requested retry");
                return Err(anyhow!("Boss requested retry"));
            }
        }
    }
    let mut merged = 0;
    let mut skipped = Vec::new();
    let first_repo_dir = resolve_repo_dir(tasks.first().and_then(|t| t.repo_path.as_deref()), tasks.first().and_then(|t| t.worktree_path.as_deref()));
    for task in &tasks {
        if task.status != TaskStatus::Done {
            skipped.push(format!("{} ({})", task.id, task.status.label()));
            continue;
        }
        let repo_dir = resolve_repo_dir(task.repo_path.as_deref(), task.worktree_path.as_deref());
        if let Some(ref branch) = task.worktree_branch {
            // Auto-commit uncommitted changes
            if let Some(wt) = task.worktree_path.as_deref()
                && std::path::Path::new(wt).exists()
            {
                auto_commit_uncommitted(wt, branch);
                sync_cargo_lock_before_merge(&repo_dir, wt, branch);
            }
            let ahead = commits_ahead(&repo_dir, branch);
            if ahead == 0 {
                aid_warn!("[aid] Warning: {} — branch {branch} has 0 commits, skipping", task.id);
                skipped.push(format!("{} (no commits)", task.id));
                continue;
            }
            if let Some(target_branch) = target {
                checkout_branch(&repo_dir, target_branch)?;
            }
            match git_merge_branch(&repo_dir, branch) {
                MergeResult::Merged => {
                    aid_info!("[aid] Merged branch {branch}");
                    run_post_merge_verify(&repo_dir, task.verify.as_deref());
                }
                MergeResult::AlreadyUpToDate => {
                    aid_warn!("[aid] Warning: {} — merge was no-op despite {ahead} commit(s)", task.id);
                    skipped.push(format!("{} (merge no-op)", task.id));
                    continue;
                }
                MergeResult::Failed(_) => {
                    aid_warn!("[aid] Warning: git merge {branch} failed, skipping {}", task.id);
                    skipped.push(format!("{} (merge conflict)", task.id));
                    continue;
                }
            }
        } else {
            aid_info!("[aid] {} — no worktree, edits applied in-place", task.id);
        }
        store.update_task_status(task.id.as_str(), TaskStatus::Merged)?;
        merged += 1;
        // Clean up worktree after successful merge
        if let Some(wt) = task.worktree_path.as_deref()
            && std::path::Path::new(wt).exists()
            && let Err(err) = remove_worktree(&repo_dir, wt)
        {
            aid_warn!("[aid] Warning: failed to clean up worktree {wt}: {err}");
        }
    }
    println!("Merged {merged} task(s) in group {group_id}");
    if !skipped.is_empty() {
        aid_info!("[aid] Skipped: {}", skipped.join(", "));
    }
    // Prune stale git worktree references
    let _ = Command::new("git")
        .args(["-C", &first_repo_dir, "worktree", "prune"])
        .output();
    Ok(())
}

fn check_single(task_id: &str, task: &Task, repo_dir: &str) -> Result<()> {
    match task.worktree_branch.as_deref() {
        Some(branch) => print_check_result(task_id, &check_merge(repo_dir, branch)),
        None => println!("{task_id}: OK (in-place edit)"),
    }
    Ok(())
}

fn check_group(group_id: &str, tasks: &[Task]) -> Result<()> {
    let mut conflicts = 0;
    for task in tasks {
        let repo_dir = resolve_repo_dir(task.repo_path.as_deref(), task.worktree_path.as_deref());
        match task.worktree_branch.as_deref() {
            Some(branch) => {
                let result = check_merge(&repo_dir, branch);
                if matches!(result, MergeCheckResult::Conflict(_)) {
                    conflicts += 1;
                }
                print_check_result(task.id.as_str(), &result);
            }
            None => println!("{}: OK (in-place edit)", task.id),
        }
    }
    println!("Checked {} task(s) in group {group_id}; conflicts: {conflicts}", tasks.len());
    Ok(())
}

fn print_check_result(task_id: &str, result: &MergeCheckResult) {
    match result {
        MergeCheckResult::Ok(commits) => println!("{task_id}: OK ({commits} commit(s))"),
        MergeCheckResult::Conflict(files) => println!("{task_id}: CONFLICT ({})", files.join(", ")),
    }
}

enum ApprovalDecision {
    Merge,
    Retry,
    Skip,
}

fn ask_approval(task: &Task) -> Result<ApprovalDecision> {
    let branch = task.worktree_branch.as_deref().unwrap_or("-");
    let prompt = format!(
        "Task {} ready to merge:\n- Agent: {}\n- Branch: {}\n\nApprove?",
        task.id,
        task.agent_display_name(),
        branch
    );
    run_approval_prompt(
        &format!("Merge:aid merge {}", task.id),
        &format!("Retry:aid retry {}", task.id),
        &prompt,
    )
}

fn ask_group_approval(group_id: &str, tasks: &[Task]) -> Result<ApprovalDecision> {
    let details = tasks
        .iter()
        .map(|task| format!("- {}: {} ({})", task.id, task.agent_display_name(), task.worktree_branch.as_deref().unwrap_or("-")))
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!("Group {group_id} ready to merge:\n{details}\n\nApprove?");
    run_approval_prompt(&format!("Merge:aid merge --group {group_id}"), "Retry", &prompt)
}

fn run_approval_prompt(merge_action: &str, retry_action: &str, prompt: &str) -> Result<ApprovalDecision> {
    let actions = format!("{merge_action},{retry_action},Skip");
    let output = match Command::new("hiboss")
        .args(["ask", "--actions", &actions, "--timeout", "300", prompt])
        .stdout(Stdio::piped())
        .output()
    {
        Ok(output) => output,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(ApprovalDecision::Merge),
        Err(err) => return Err(err.into()),
    };
    let reply = String::from_utf8_lossy(&output.stdout);
    if reply.contains("Skip") {
        return Ok(ApprovalDecision::Skip);
    }
    if reply.contains("Retry") {
        return Ok(ApprovalDecision::Retry);
    }
    Ok(ApprovalDecision::Merge)
}

#[cfg(test)]
mod tests;
