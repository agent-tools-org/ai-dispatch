// Handler for `aid audit <id>` — show detailed task view with events.
// Includes git diff if the task used a worktree.

use anyhow::Result;
use std::sync::Arc;

use crate::board::render_task_detail;
use crate::store::Store;

pub fn run(store: &Arc<Store>, task_id: &str) -> Result<()> {
    let task = store.get_task(task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task '{}' not found", task_id))?;
    let events = store.get_events(task_id)?;

    print!("{}", render_task_detail(&task, &events));

    // Show git diff if worktree exists
    if let Some(ref wt_path) = task.worktree_path {
        if std::path::Path::new(wt_path).exists() {
            println!("\nChanges:");
            let output = std::process::Command::new("git")
                .args(["-C", wt_path, "diff", "--stat", "HEAD~1"])
                .output();
            match output {
                Ok(o) if o.status.success() => {
                    print!("{}", String::from_utf8_lossy(&o.stdout));
                }
                _ => println!("  (could not read git diff)"),
            }
        }
    }
    Ok(())
}
