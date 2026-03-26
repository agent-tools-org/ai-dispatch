// Shared helpers for `aid show` — task loading, context reconstruction, diff stats,
// change detection, research output, and stderr tailing.
// Exports: `load_task` (pub(crate)). Private helpers used by `show.rs` via `pub(super)`.
// Deps: `crate::cmd`, `crate::paths`, `crate::skills`, `crate::store`, `crate::types`,
// `crate::workgroup`, `crate::templates`; uses `super::extract_messages_from_log`.

use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

use crate::paths;
use crate::store::Store;
use crate::types::{Task, VerifyStatus};

use super::extract_messages_from_log;

pub(crate) fn load_task(store: &Arc<Store>, task_id: &str) -> Result<Task> {
    store
        .get_task(task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task '{task_id}' not found"))
}

pub(super) fn reconstruct_context(store: &Arc<Store>, task: &Task) -> Result<(String, String)> {
    let workgroup = if let Some(group_id) = task.workgroup_id.as_deref() {
        store.get_workgroup(group_id)?
    } else {
        None
    };
    let milestones = if let Some(group_id) = task.workgroup_id.as_deref() {
        store.get_workgroup_milestones(group_id)?
    } else {
        vec![]
    };
    let skill_names = crate::skills::auto_skills(&task.agent, task.worktree_path.is_some());
    let skill_parts = skill_names
        .iter()
        .map(|skill| {
            crate::skills::resolve_skill_content(skill)
                .unwrap_or_else(|err| format!("[missing skill: {skill}: {err}]"))
        })
        .collect::<Vec<_>>();
    let skill_content = skill_parts.join("\n\n");
    let mut resolved_prompt = crate::workgroup::compose_prompt(
        &task.prompt,
        None,
        workgroup.as_ref(),
        &milestones,
        &[],
    );
    if !skill_content.is_empty() {
        resolved_prompt = format!("{resolved_prompt}\n\n--- Methodology ---\n{skill_content}");
    }
    resolved_prompt = crate::templates::inject_milestone_prompt(&resolved_prompt);
    Ok((skill_content, resolved_prompt))
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
    if task.verify_status == VerifyStatus::EmptyDiff {
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
    extract_messages_from_log(&log_path, false).filter(|messages| !messages.is_empty())
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
