// Shared task and output helpers for aid show.
// Loads stored tasks, detects changes, and renders failure/research details.
// Depends on Store, paths, Task, and output parsing.

use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

use crate::paths;
use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent};

use super::extract_messages_from_log;

pub(crate) fn load_task(store: &Arc<Store>, task_id: &str) -> Result<Task> {
    store
        .get_task(task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task '{task_id}' not found"))
}

pub(super) fn inplace_diff_stat(repo_path: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["-C", repo_path, "diff", "--stat"])
        .output()
        .ok()?;
    if output.status.success() && !output.stdout.is_empty() {
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        None
    }
}

pub(super) fn task_has_changes(task: &Task) -> bool {
    if task
        .delivery_assessment()
        .is_some_and(|delivery| delivery.implies_no_changes())
    {
        return false;
    }
    task.worktree_path
        .as_ref()
        .is_some_and(|path| Path::new(path).exists())
        || (task.worktree_branch.is_none()
            && inplace_diff_stat(task.repo_path.as_deref().unwrap_or(".")).is_some())
}

pub(super) fn completion_conclusion(store: &Store, task_id: &str) -> Option<String> {
    let summary_json = store.get_completion_summary(task_id).ok()??;
    let summary =
        serde_json::from_str::<crate::cmd::summary::CompletionSummary>(&summary_json).ok()?;
    if summary.conclusion.is_empty() {
        None
    } else {
        Some(summary.conclusion)
    }
}

pub(super) fn research_findings(store: &Store, task: &Task) -> Option<String> {
    if let Some(conclusion) = completion_conclusion(store, task.id.as_str()) {
        return Some(conclusion);
    }
    let log_path = task
        .log_path
        .as_ref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::paths::log_path(task.id.as_str()));
    extract_messages_from_log(&log_path, false, Some(task.agent_display_name())).filter(|messages| !messages.is_empty())
}

pub(super) fn stderr_tail(task_id: &str) -> Option<String> {
    let content = std::fs::read_to_string(paths::stderr_path(task_id)).ok()?;
    if content.is_empty() {
        return None;
    }
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(20);
    let mut out = String::new();
    if start > 0 {
        out.push_str(&format!("  ... ({start} lines omitted)\n"));
    }
    for line in &lines[start..] {
        out.push_str(&format!("  {line}\n"));
    }
    Some(out)
}

pub(super) fn failure_details(events: &[TaskEvent]) -> Option<String> {
    let errors: Vec<&TaskEvent> = events
        .iter()
        .filter(|event| event.event_kind == EventKind::Error)
        .collect();
    if errors.is_empty() {
        return None;
    }
    let start = errors.len().saturating_sub(3);
    let mut out = String::new();
    for event in &errors[start..] {
        for line in event.detail.lines() {
            out.push_str("  ");
            out.push_str(line);
            out.push('\n');
        }
    }
    Some(out)
}
