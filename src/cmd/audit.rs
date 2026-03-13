// Handler for `aid audit <id>` — show detailed task view with events and stderr.
// Includes git diff if the task used a worktree, stderr on failure.

use anyhow::Result;
use std::sync::Arc;

use crate::board::render_task_detail;
use crate::paths;
use crate::store::Store;
use crate::types::TaskStatus;

pub fn run(store: &Arc<Store>, task_id: &str) -> Result<()> {
    print!("{}", audit_text(store, task_id)?);
    Ok(())
}

pub fn audit_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = store.get_task(task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task '{}' not found", task_id))?;
    let events = store.get_events(task_id)?;
    let mut out = render_task_detail(&task, &events);

    if task.status == TaskStatus::Failed
        && let Some(stderr) = stderr_text(task_id)
    {
        out.push_str("\nStderr:\n");
        out.push_str(&stderr);
    }

    if let Some(ref wt_path) = task.worktree_path
        && std::path::Path::new(wt_path).exists()
    {
        out.push_str("\nChanges:\n");
        out.push_str(&git_diff_text(wt_path));
    }

    Ok(out)
}

fn stderr_text(task_id: &str) -> Option<String> {
    let stderr_path = paths::stderr_path(task_id);
    let content = std::fs::read_to_string(stderr_path).ok()?;
    if content.is_empty() {
        return None;
    }

    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(20);
    let mut out = String::new();
    if start > 0 {
        out.push_str(&format!("  ... ({} lines omitted)\n", start));
    }
    for line in &lines[start..] {
        out.push_str(&format!("  {line}\n"));
    }
    Some(out)
}

fn git_diff_text(wt_path: &str) -> String {
    let output = std::process::Command::new("git")
        .args(["-C", wt_path, "diff", "--stat", "HEAD~1"])
        .output();
    match output {
        Ok(result) if result.status.success() => String::from_utf8_lossy(&result.stdout).into(),
        _ => "  (could not read git diff)\n".to_string(),
    }
}
