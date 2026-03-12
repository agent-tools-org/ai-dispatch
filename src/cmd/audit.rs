// Handler for `aid audit <id>` — show detailed task view with events and stderr.
// Includes git diff if the task used a worktree, stderr on failure.

use anyhow::Result;
use std::sync::Arc;

use crate::board::render_task_detail;
use crate::paths;
use crate::store::Store;
use crate::types::TaskStatus;

pub fn run(store: &Arc<Store>, task_id: &str) -> Result<()> {
    let task = store.get_task(task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task '{}' not found", task_id))?;
    let events = store.get_events(task_id)?;

    print!("{}", render_task_detail(&task, &events));

    // Show stderr on failure
    if task.status == TaskStatus::Failed {
        let stderr_path = paths::stderr_path(task_id);
        if stderr_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&stderr_path) {
                if !content.is_empty() {
                    println!("\nStderr:");
                    // Show last 20 lines to avoid flooding
                    let lines: Vec<&str> = content.lines().collect();
                    let start = lines.len().saturating_sub(20);
                    if start > 0 {
                        println!("  ... ({} lines omitted)", start);
                    }
                    for line in &lines[start..] {
                        println!("  {}", line);
                    }
                }
            }
        }
    }

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
