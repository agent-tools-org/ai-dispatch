// Handler for `aid merge` — mark a done task as merged.
// Exports run(); depends on Store task lookups and TaskStatus transitions.

use anyhow::{Result, anyhow};
use std::sync::Arc;

use crate::store::Store;
use crate::types::TaskStatus;

pub fn run(store: Arc<Store>, task_id: &str) -> Result<()> {
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
