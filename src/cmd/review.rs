// Handler for `aid review <task-id>` — show worktree diff and task events.
// Prints task summary, diff stat, event timeline, and full diff for manual review.

use anyhow::Result;
use std::process::Command;
use std::sync::Arc;

use crate::store::Store;
use crate::types::EventKind;

pub struct ReviewArgs {
    pub task_id: String,
}

pub fn run(store: &Arc<Store>, args: ReviewArgs) -> Result<()> {
    print!("{}", review_text(store, &args.task_id)?);
    Ok(())
}

pub fn review_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = store
        .get_task(task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task '{}' not found", task_id))?;
    let wt_path = task
        .worktree_path
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Task has no worktree — nothing to review"))?;

    let wt = std::path::Path::new(wt_path);
    if !wt.exists() {
        anyhow::bail!("Worktree no longer exists: {wt_path}");
    }

    let mut out = String::new();
    out.push_str(&format!("=== Review: {} ===\n", task.id));
    out.push_str(&format!(
        "Agent: {}  Status: {}  Prompt: {}\n",
        task.agent,
        task.status.label(),
        truncate(&task.prompt, 60),
    ));
    if let Some(ref model) = task.model {
        out.push_str(&format!("Model: {model}\n"));
    }

    out.push_str("\n--- Diff Stat ---\n");
    out.push_str(&diff_stat_text(wt_path));

    let events = store.get_events(task_id)?;
    if !events.is_empty() {
        out.push_str("\n--- Events (last 10) ---\n");
        let start = events.len().saturating_sub(10);
        for event in &events[start..] {
            let kind = event.event_kind.as_str();
            let time = event.timestamp.format("%H:%M:%S");
            let detail = truncate(&event.detail, 80);
            let marker = if event.event_kind == EventKind::Error { "!" } else { " " };
            out.push_str(&format!("{marker} [{time}] {kind}: {detail}\n"));
        }
    }

    out.push_str("\n--- Full Diff ---\n");
    out.push_str(&full_diff_text(wt_path));
    out.push_str(&format!("\nWorktree: {wt_path}\n"));
    Ok(out)
}

fn diff_stat_text(wt_path: &str) -> String {
    match git_output(wt_path, &["diff", "--stat"]) {
        Some(output) if !output.trim().is_empty() => output,
        Some(_) => git_output(wt_path, &["diff", "--stat", "HEAD~1"])
            .unwrap_or_else(|| "  (no changes detected)\n".to_string()),
        None => "  (could not read git diff)\n".to_string(),
    }
}

fn full_diff_text(wt_path: &str) -> String {
    match git_output(wt_path, &["diff"]) {
        Some(output) if !output.trim().is_empty() => output,
        Some(_) => git_output(wt_path, &["diff", "HEAD~1"])
            .unwrap_or_else(|| "  (no diff available)\n".to_string()),
        None => "  (could not read git diff)\n".to_string(),
    }
}

fn git_output(wt_path: &str, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(wt_path)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}
