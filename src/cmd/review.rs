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
    let task = store
        .get_task(&args.task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task '{}' not found", args.task_id))?;

    println!("=== Review: {} ===", task.id);
    println!("Agent: {}  Status: {}  Prompt: {}", task.agent, task.status.label(), truncate(&task.prompt, 60));
    if let Some(ref model) = task.model {
        println!("Model: {model}");
    }

    // Worktree diff
    let wt_path = task
        .worktree_path
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Task has no worktree — nothing to review"))?;

    let wt = std::path::Path::new(wt_path);
    if !wt.exists() {
        anyhow::bail!("Worktree no longer exists: {wt_path}");
    }

    // Diff stat
    println!("\n--- Diff Stat ---");
    let stat_out = Command::new("git")
        .args(["-C", wt_path, "diff", "--stat"])
        .output();
    match stat_out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            if s.trim().is_empty() {
                // Try against base branch
                let stat_base = Command::new("git")
                    .args(["-C", wt_path, "diff", "--stat", "HEAD~1"])
                    .output();
                match stat_base {
                    Ok(o2) if o2.status.success() => print!("{}", String::from_utf8_lossy(&o2.stdout)),
                    _ => println!("  (no changes detected)"),
                }
            } else {
                print!("{s}");
            }
        }
        _ => println!("  (could not read git diff)"),
    }

    // Event timeline (last 10)
    let events = store.get_events(&args.task_id)?;
    if !events.is_empty() {
        println!("\n--- Events (last 10) ---");
        let start = events.len().saturating_sub(10);
        for event in &events[start..] {
            let kind = event.event_kind.as_str();
            let time = event.timestamp.format("%H:%M:%S");
            let detail = truncate(&event.detail, 80);
            let marker = if event.event_kind == EventKind::Error { "!" } else { " " };
            println!("{marker} [{time}] {kind}: {detail}");
        }
    }

    // Full diff
    println!("\n--- Full Diff ---");
    let diff_out = Command::new("git")
        .args(["-C", wt_path, "diff"])
        .output();
    match diff_out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            if s.trim().is_empty() {
                // Try committed changes
                let diff_base = Command::new("git")
                    .args(["-C", wt_path, "diff", "HEAD~1"])
                    .output();
                match diff_base {
                    Ok(o2) if o2.status.success() => print!("{}", String::from_utf8_lossy(&o2.stdout)),
                    _ => println!("  (no diff available)"),
                }
            } else {
                print!("{s}");
            }
        }
        _ => println!("  (could not read git diff)"),
    }

    println!("\nWorktree: {wt_path}");
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}
