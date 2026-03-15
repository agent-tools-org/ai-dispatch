// Handler for `aid merge` — mark done task(s) as merged, optionally by workgroup.
// Exports: run()
// Deps: crate::store::Store, crate::types::TaskStatus

use anyhow::{anyhow, Result};
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
    // Pre-merge verification: run verify command in worktree
    if let Some(wt) = task.worktree_path.as_deref() {
        if std::path::Path::new(wt).exists() {
            let verify_cmd = task.verify.as_deref().unwrap_or("cargo check");
            let parts: Vec<&str> = verify_cmd.split_whitespace().collect();
            if let Some((cmd, args)) = parts.split_first() {
                let output = std::process::Command::new(cmd)
                    .args(args)
                    .current_dir(wt)
                    .output();
                match output {
                    Ok(o) if !o.status.success() => {
                        eprintln!("[aid] Warning: `{}` failed in worktree {}", verify_cmd, wt);
                        let stderr = String::from_utf8_lossy(&o.stderr);
                        for line in stderr.lines().take(5) {
                            eprintln!("  {}", line);
                        }
                    }
                    Err(e) => eprintln!("[aid] Warning: could not run `{}`: {}", verify_cmd, e),
                    _ => {}
                }
            }
        }
    }
    // Auto cherry-pick worktree branch into current branch
    if let Some(ref branch) = task.worktree_branch {
        let repo_dir = task.repo_path.as_deref().unwrap_or(".");
        let output = std::process::Command::new("git")
            .args(["merge", branch, "--no-edit"])
            .current_dir(repo_dir)
            .output();
        match output {
            Ok(o) if o.status.success() => {
                eprintln!("[aid] Merged branch {branch} into current branch");
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                eprintln!("[aid] Warning: git merge {branch} failed:");
                for line in stderr.lines().take(5) {
                    eprintln!("  {}", line);
                }
                eprintln!("[aid] Manual merge needed: git merge {branch}");
            }
            Err(e) => eprintln!("[aid] Warning: could not run git merge: {e}"),
        }
    } else {
        eprintln!(
            "[aid] No worktree branch — agent edited files in-place. Nothing to merge."
        );
    }
    store.update_task_status(task_id, TaskStatus::Merged)?;
    println!("Marked {task_id} as merged");
    if let Some(wt) = task.worktree_path.as_deref() {
        if std::path::Path::new(wt).exists() {
            match std::fs::remove_dir_all(wt) {
                Ok(()) => eprintln!("[aid] Cleaned up worktree {wt}"),
                Err(e) => eprintln!("[aid] Warning: failed to remove {wt}: {e}"),
            }
        }
    }
    if let Some(repo) = task.repo_path.as_deref() {
        let _ = std::process::Command::new("git")
            .args(["worktree", "prune"])
            .current_dir(repo)
            .output();
    }
    Ok(())
}

fn merge_group(store: &Store, group_id: &str) -> Result<()> {
    let tasks = store.list_tasks_by_group(group_id)?;
    if tasks.is_empty() {
        return Err(anyhow!("No tasks found in group '{group_id}'"));
    }
    let mut merged = 0;
    let mut skipped = Vec::new();
    let repo_dir = tasks.first().and_then(|t| t.repo_path.as_deref()).unwrap_or(".");
    for task in &tasks {
        if task.status == TaskStatus::Done {
            // Auto cherry-pick worktree branch
            if let Some(ref branch) = task.worktree_branch {
                let output = std::process::Command::new("git")
                    .args(["merge", branch, "--no-edit"])
                    .current_dir(repo_dir)
                    .output();
                match output {
                    Ok(o) if o.status.success() => {
                        eprintln!("[aid] Merged branch {branch}");
                    }
                    Ok(_) => {
                        eprintln!("[aid] Warning: git merge {branch} failed, skipping {}", task.id);
                        skipped.push(format!("{} (merge conflict)", task.id));
                        continue;
                    }
                    Err(e) => {
                        eprintln!("[aid] Warning: could not run git merge: {e}");
                    }
                }
            } else {
                eprintln!("[aid] {} — no worktree, edits applied in-place", task.id);
            }
            store.update_task_status(task.id.as_str(), TaskStatus::Merged)?;
            merged += 1;
            if let Some(wt) = task.worktree_path.as_deref() {
                if std::path::Path::new(wt).exists() {
                    match std::fs::remove_dir_all(wt) {
                        Ok(()) => eprintln!("[aid] Removed worktree {wt}"),
                        Err(e) => eprintln!("[aid] Warning: failed to remove {wt}: {e}"),
                    }
                }
            }
        } else {
            skipped.push(format!("{} ({})", task.id, task.status.label()));
        }
    }
    println!("Merged {merged} task(s) in group {group_id}");
    if !skipped.is_empty() {
        eprintln!("[aid] Skipped (not done): {}", skipped.join(", "));
    }
    // Prune stale git worktree references
    if let Some(task) = tasks.first() {
        if let Some(repo) = task.repo_path.as_deref() {
            let _ = std::process::Command::new("git")
                .args(["worktree", "prune"])
                .current_dir(repo)
                .output();
        }
    }
    Ok(())
}
