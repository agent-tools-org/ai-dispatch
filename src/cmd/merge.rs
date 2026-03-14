// Handler for `aid merge` — mark done task(s) as merged, optionally by workgroup.
// Exports: run()
// Deps: crate::store::Store, crate::types::TaskStatus

use anyhow::{Result, anyhow};
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
    let task = store.get_task(task_id)?
        .ok_or_else(|| anyhow!("Task '{task_id}' not found"))?;
    if task.status != TaskStatus::Done {
        return Err(anyhow!("Task '{task_id}' is {} — only DONE tasks can be marked as merged", task.status.label()));
    }
    store.update_task_status(task_id, TaskStatus::Merged)?;
    println!("Marked {task_id} as merged");
    if let Some(wt) = task.worktree_path.as_deref() {
        if std::path::Path::new(wt).exists() {
            eprintln!("[aid] Tip: worktree still exists at {wt}");
        }
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
    for task in &tasks {
        if task.status == TaskStatus::Done {
            store.update_task_status(task.id.as_str(), TaskStatus::Merged)?;
            merged += 1;
            // Clean up worktree if it exists
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
