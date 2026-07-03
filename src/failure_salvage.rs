// Best-effort preservation for failed worktree tasks.
// Exports a deterministic salvage hook that writes partial-work.md and WIP commits.
// Deps: Store task/events, git CLI, paths, worktree live-state.

use anyhow::{Context, Result};
use chrono::Local;
use std::path::Path;
use std::process::Command;

use crate::store::Store;
use crate::types::{EventKind, TaskEvent, TaskId, TaskStatus};

const PARTIAL_WORK_FILE: &str = "partial-work.md";

pub(crate) fn salvage_failed_task(store: &Store, task_id: &TaskId) {
    match try_salvage_failed_task(store, task_id) {
        Ok(SalvageOutcome::Skipped) | Ok(SalvageOutcome::Saved) => {}
        Err(err) => aid_warn!("[aid] Warning: failed to salvage partial work for {task_id}: {err}"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SalvageOutcome {
    Skipped,
    Saved,
}

fn try_salvage_failed_task(store: &Store, task_id: &TaskId) -> Result<SalvageOutcome> {
    let Some(task) = store.get_task(task_id.as_str())? else {
        return Ok(SalvageOutcome::Skipped);
    };
    if task.status != TaskStatus::Failed || task.read_only {
        return Ok(SalvageOutcome::Skipped);
    }
    let Some(worktree_path) = task.worktree_path.as_deref() else {
        return Ok(SalvageOutcome::Skipped);
    };
    let worktree = Path::new(worktree_path);
    if !worktree.exists() {
        return Ok(SalvageOutcome::Skipped);
    }
    let state = crate::worktree::capture_live_worktree_state(worktree)?;
    if !state.is_dirty() {
        return Ok(SalvageOutcome::Skipped);
    }
    let events = store.get_events(task_id.as_str())?;
    write_partial_work(task_id, worktree_path, &state, &events)?;
    commit_partial_work(worktree, task_id.as_str())?;
    Ok(SalvageOutcome::Saved)
}

fn write_partial_work(
    task_id: &TaskId,
    worktree_path: &str,
    state: &crate::worktree::LiveWorktreeState,
    events: &[TaskEvent],
) -> Result<()> {
    let task_dir = crate::paths::task_dir(task_id.as_str());
    std::fs::create_dir_all(&task_dir)?;
    let mut out = String::new();
    out.push_str(&format!("# Partial Work Salvage for {task_id}\n\n"));
    out.push_str(&format!("Worktree: {worktree_path}\n"));
    out.push_str(&format!("Captured: {}\n\n", Local::now().to_rfc3339()));
    out.push_str("## Git Status Summary\n\n");
    out.push_str(&state.summary_text());
    out.push_str("\n\n## Diff Stat Including Untracked\n\n");
    out.push_str(&state.dirty_stat_text());
    out.push_str("\n## Recent Activity Before Failure\n\n");
    append_recent_activity(&mut out, events);
    std::fs::write(task_dir.join(PARTIAL_WORK_FILE), out)
        .with_context(|| format!("Failed to write {}", task_dir.join(PARTIAL_WORK_FILE).display()))?;
    Ok(())
}

fn append_recent_activity(out: &mut String, events: &[TaskEvent]) {
    let mut recent = events
        .iter()
        .rev()
        .filter(|event| matches!(event.event_kind, EventKind::Milestone | EventKind::ToolCall))
        .take(5)
        .collect::<Vec<_>>();
    recent.reverse();
    if recent.is_empty() {
        out.push_str("(no milestone/tool_call events recorded)\n");
        return;
    }
    for event in recent {
        out.push_str(&format!(
            "- [{}] {}: {}\n",
            event.timestamp.format("%H:%M:%S"),
            event.event_kind.as_str(),
            event.detail
        ));
    }
}

fn commit_partial_work(worktree: &Path, task_id: &str) -> Result<()> {
    run_git(worktree, &["add", "-A"])?;
    if !has_staged_changes(worktree)? {
        return Ok(());
    }
    let message = format!("wip: partial work salvage (task {task_id} failed)");
    run_git(worktree, &["commit", "-m", &message])?;
    Ok(())
}

fn has_staged_changes(worktree: &Path) -> Result<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(["diff", "--cached", "--quiet"])
        .output()
        .context("Failed to run git diff --cached --quiet")?;
    match output.status.code() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => anyhow::bail!(
            "git diff --cached --quiet failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

fn run_git(worktree: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(args)
        .output()
        .with_context(|| format!("Failed to run git {}", args.join(" ")))?;
    anyhow::ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[cfg(test)]
#[path = "failure_salvage_tests.rs"]
mod tests;
