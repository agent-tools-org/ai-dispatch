// Handler for `aid merge` — mark done task(s) as merged, optionally by workgroup.
// Exports: run()
// Deps: crate::store::Store, crate::types::TaskStatus

use anyhow::{anyhow, Result};
use std::process::Command;
use std::sync::Arc;

use crate::store::Store;
use crate::types::TaskStatus;

pub fn run(store: Arc<Store>, task_id: Option<&str>, group: Option<&str>) -> Result<()> {
    match (task_id, group) {
        (Some(id), _) => merge_single(&store, id),
        (_, Some(group_id)) => merge_group(&store, group_id),
        (None, None) => Err(anyhow!("Provide either a task ID or --group <wg-id>")),
    }
}

/// Resolve repo directory: prefer task.repo_path, then detect from worktree, fallback to ".".
fn resolve_repo_dir(repo_path: Option<&str>, worktree_path: Option<&str>) -> String {
    if let Some(repo) = repo_path {
        return repo.to_string();
    }
    // Detect repo from worktree's git config (worktrees link back to main repo)
    if let Some(wt) = worktree_path {
        if let Ok(out) = Command::new("git")
            .args(["-C", wt, "rev-parse", "--show-toplevel"])
            .output()
        {
            if out.status.success() {
                let toplevel = String::from_utf8_lossy(&out.stdout).trim().to_string();
                // Worktree's toplevel IS the worktree itself; get the main repo via commondir
                if let Ok(common) = Command::new("git")
                    .args(["-C", wt, "rev-parse", "--git-common-dir"])
                    .output()
                {
                    if common.status.success() {
                        let common_dir = String::from_utf8_lossy(&common.stdout).trim().to_string();
                        let common_path = std::path::Path::new(&common_dir);
                        // commondir points to main repo's .git — parent is the repo root
                        if let Some(parent) = common_path.parent() {
                            if parent.join(".git").exists() {
                                return parent.to_string_lossy().to_string();
                            }
                        }
                    }
                }
                return toplevel;
            }
        }
    }
    ".".to_string()
}

/// Count commits on branch ahead of base (main or HEAD of repo).
fn commits_ahead(repo_dir: &str, branch: &str) -> u32 {
    let out = Command::new("git")
        .args(["-C", repo_dir, "rev-list", "--count", &format!("HEAD..{branch}")])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(0)
        }
        _ => 0,
    }
}

/// Check for uncommitted changes in a worktree and auto-commit them.
fn auto_commit_uncommitted(wt_path: &str, branch: &str) -> bool {
    // Check for any uncommitted changes (staged + unstaged + untracked)
    let status = Command::new("git")
        .args(["-C", wt_path, "status", "--porcelain"])
        .output();
    let has_changes = match status {
        Ok(o) if o.status.success() => !o.stdout.is_empty(),
        _ => return false,
    };
    if !has_changes {
        return false;
    }
    eprintln!("[aid] Worktree has uncommitted changes — auto-committing on {branch}");
    // Stage all changes
    let _ = Command::new("git")
        .args(["-C", wt_path, "add", "-A"])
        .output();
    // Commit
    let out = Command::new("git")
        .args(["-C", wt_path, "commit", "-m", "chore: auto-commit agent changes before merge"])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            eprintln!("[aid] Auto-committed uncommitted changes");
            true
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            eprintln!("[aid] Warning: auto-commit failed: {}", stderr.lines().next().unwrap_or(""));
            false
        }
        Err(e) => {
            eprintln!("[aid] Warning: auto-commit failed: {e}");
            false
        }
    }
}

/// Perform git merge and return whether new commits were actually merged.
fn git_merge_branch(repo_dir: &str, branch: &str) -> MergeResult {
    let head_before = Command::new("git")
        .args(["-C", repo_dir, "rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    let output = Command::new("git")
        .args(["-C", repo_dir, "merge", branch, "--no-edit"])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if stdout.contains("Already up to date") {
                return MergeResult::AlreadyUpToDate;
            }
            // Verify HEAD actually moved
            let head_after = Command::new("git")
                .args(["-C", repo_dir, "rev-parse", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
            if head_before == head_after {
                return MergeResult::AlreadyUpToDate;
            }
            MergeResult::Merged
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            MergeResult::Failed(stderr)
        }
        Err(e) => MergeResult::Failed(e.to_string()),
    }
}

enum MergeResult {
    Merged,
    AlreadyUpToDate,
    Failed(String),
}

/// Remove a worktree properly using git, with fs::remove_dir_all as fallback.
pub fn remove_worktree(repo_dir: &str, wt_path: &str) {
    // Try `git worktree remove` first (proper cleanup)
    let result = Command::new("git")
        .args(["-C", repo_dir, "worktree", "remove", "--force", wt_path])
        .output();
    match result {
        Ok(o) if o.status.success() => {
            eprintln!("[aid] Cleaned up worktree {wt_path}");
            return;
        }
        _ => {}
    }
    // Fallback to manual removal + prune
    match std::fs::remove_dir_all(wt_path) {
        Ok(()) => {
            eprintln!("[aid] Cleaned up worktree {wt_path}");
            let _ = Command::new("git")
                .args(["-C", repo_dir, "worktree", "prune"])
                .output();
        }
        Err(e) => eprintln!("[aid] Warning: failed to remove {wt_path}: {e}"),
    }
}

fn merge_single(store: &Store, task_id: &str) -> Result<()> {
    let task = store
        .get_task(task_id)?
        .ok_or_else(|| anyhow!("Task '{task_id}' not found"))?;
    if task.status != TaskStatus::Done {
        return Err(anyhow!(
            "Task '{task_id}' is {} — only DONE tasks can be marked as merged",
            task.status.label()
        ));
    }
    let repo_dir = resolve_repo_dir(task.repo_path.as_deref(), task.worktree_path.as_deref());

    // Pre-merge verification: run verify command in worktree
    if let Some(wt) = task.worktree_path.as_deref() {
        if std::path::Path::new(wt).exists() {
            run_verify_in_worktree(wt, task.verify.as_deref());
        }
    }
    // Auto cherry-pick worktree branch into current branch
    if let Some(ref branch) = task.worktree_branch {
        // Auto-commit any uncommitted changes before merge
        if let Some(wt) = task.worktree_path.as_deref() {
            if std::path::Path::new(wt).exists() {
                auto_commit_uncommitted(wt, branch);
            }
        }
        // Pre-check: verify branch has commits to merge
        let ahead = commits_ahead(&repo_dir, branch);
        if ahead == 0 {
            eprintln!("[aid] Error: branch {branch} has 0 commits ahead — nothing to merge");
            eprintln!("[aid] The agent may not have committed its changes.");
            if let Some(wt) = task.worktree_path.as_deref() {
                if std::path::Path::new(wt).exists() {
                    eprintln!("[aid] Worktree preserved at {wt} for manual recovery");
                }
            }
            return Err(anyhow!("No commits to merge from {branch}"));
        }
        eprintln!("[aid] Branch {branch} has {ahead} commit(s) ahead");
        match git_merge_branch(&repo_dir, branch) {
            MergeResult::Merged => {
                eprintln!("[aid] Merged branch {branch} into current branch");
            }
            MergeResult::AlreadyUpToDate => {
                eprintln!("[aid] Error: git merge reported 'Already up to date' despite {ahead} commit(s)");
                eprintln!("[aid] This may indicate a repo path mismatch. Worktree preserved.");
                return Err(anyhow!("Merge was a no-op — possible repo_path mismatch"));
            }
            MergeResult::Failed(stderr) => {
                eprintln!("[aid] Warning: git merge {branch} failed:");
                for line in stderr.lines().take(5) {
                    eprintln!("  {}", line);
                }
                eprintln!("[aid] Manual merge needed: git merge {branch}");
                // Don't clean up worktree — user needs it for manual merge
                store.update_task_status(task_id, TaskStatus::Done)?;
                return Err(anyhow!("Merge failed — resolve manually, then re-run aid merge {task_id}"));
            }
        }
    } else {
        eprintln!(
            "[aid] No worktree branch — agent edited files in-place. Nothing to merge."
        );
    }
    store.update_task_status(task_id, TaskStatus::Merged)?;
    println!("Marked {task_id} as merged");
    // Clean up worktree only after successful merge
    if let Some(wt) = task.worktree_path.as_deref() {
        if std::path::Path::new(wt).exists() {
            remove_worktree(&repo_dir, wt);
        }
    }
    Ok(())
}

fn run_verify_in_worktree(wt: &str, verify: Option<&str>) {
    let verify_cmd = match verify {
        Some("auto") | None => "cargo check",
        Some(cmd) => cmd,
    };
    let parts: Vec<&str> = verify_cmd.split_whitespace().collect();
    let Some((cmd, args)) = parts.split_first() else { return };
    let output = Command::new(cmd).args(args).current_dir(wt).output();
    match output {
        Ok(o) if !o.status.success() => {
            eprintln!("[aid] Warning: `{verify_cmd}` failed in worktree {wt}");
            let stderr = String::from_utf8_lossy(&o.stderr);
            for line in stderr.lines().take(5) {
                eprintln!("  {}", line);
            }
        }
        Err(e) => eprintln!("[aid] Warning: could not run `{verify_cmd}`: {e}"),
        _ => {}
    }
}

fn merge_group(store: &Store, group_id: &str) -> Result<()> {
    let tasks = store.list_tasks_by_group(group_id)?;
    if tasks.is_empty() {
        return Err(anyhow!("No tasks found in group '{group_id}'"));
    }
    let mut merged = 0;
    let mut skipped = Vec::new();
    let first_repo_dir = resolve_repo_dir(
        tasks.first().and_then(|t| t.repo_path.as_deref()),
        tasks.first().and_then(|t| t.worktree_path.as_deref()),
    );
    for task in &tasks {
        if task.status != TaskStatus::Done {
            skipped.push(format!("{} ({})", task.id, task.status.label()));
            continue;
        }
        let repo_dir = resolve_repo_dir(task.repo_path.as_deref(), task.worktree_path.as_deref());
        if let Some(ref branch) = task.worktree_branch {
            // Auto-commit uncommitted changes
            if let Some(wt) = task.worktree_path.as_deref() {
                if std::path::Path::new(wt).exists() {
                    auto_commit_uncommitted(wt, branch);
                }
            }
            let ahead = commits_ahead(&repo_dir, branch);
            if ahead == 0 {
                eprintln!("[aid] Warning: {} — branch {branch} has 0 commits, skipping", task.id);
                skipped.push(format!("{} (no commits)", task.id));
                continue;
            }
            match git_merge_branch(&repo_dir, branch) {
                MergeResult::Merged => {
                    eprintln!("[aid] Merged branch {branch}");
                }
                MergeResult::AlreadyUpToDate => {
                    eprintln!("[aid] Warning: {} — merge was no-op despite {ahead} commit(s)", task.id);
                    skipped.push(format!("{} (merge no-op)", task.id));
                    continue;
                }
                MergeResult::Failed(_) => {
                    eprintln!("[aid] Warning: git merge {branch} failed, skipping {}", task.id);
                    skipped.push(format!("{} (merge conflict)", task.id));
                    continue;
                }
            }
        } else {
            eprintln!("[aid] {} — no worktree, edits applied in-place", task.id);
        }
        store.update_task_status(task.id.as_str(), TaskStatus::Merged)?;
        merged += 1;
        // Clean up worktree after successful merge
        if let Some(wt) = task.worktree_path.as_deref() {
            if std::path::Path::new(wt).exists() {
                remove_worktree(&repo_dir, wt);
            }
        }
    }
    println!("Merged {merged} task(s) in group {group_id}");
    if !skipped.is_empty() {
        eprintln!("[aid] Skipped: {}", skipped.join(", "));
    }
    // Prune stale git worktree references
    let _ = Command::new("git")
        .args(["-C", &first_repo_dir, "worktree", "prune"])
        .output();
    Ok(())
}
