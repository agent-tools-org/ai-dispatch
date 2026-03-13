// Handler for `aid review <task-id>` — show task artifacts and events.
// Prefers worktree diffs, then falls back to output files or raw logs.

use anyhow::Result;
use std::process::Command;
use std::sync::Arc;

use crate::cmd::output;
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

    if let Some(ref worktree_path) = task.worktree_path {
        let wt = std::path::Path::new(worktree_path);
        if wt.exists() {
            out.push_str("\n--- Diff Stat ---\n");
            out.push_str(&diff_stat_text(worktree_path));
            out.push_str("\n--- Full Diff ---\n");
            out.push_str(&full_diff_text(worktree_path));
            out.push_str(&format!("\nWorktree: {worktree_path}\n"));
            return Ok(out);
        }
    }

    if let Ok(task_output) = output::read_task_output(&task) {
        out.push_str("\n--- Output ---\n");
        out.push_str(&task_output);
        return Ok(out);
    }

    if let Some(ref log_path) = task.log_path
        && let Ok(log) = std::fs::read_to_string(log_path)
    {
        out.push_str("\n--- Log ---\n");
        out.push_str(&log);
        return Ok(out);
    }

    out.push_str("\n--- Artifacts ---\n  (no worktree diff or output file available)\n");
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
