// JSON serialization for `aid show --json` and webhook payloads (`task_hook_json`).
// Exports: `task_json`, `task_hook_json` (pub(crate)).
// Deps: `serde_json`, `crate::cmd`, `crate::store`, `crate::types`; uses `show_helpers::load_task`
// and `show_output` re-exports (`diff_stat`, `parse_diff_stat`, `output_text_for_task`).

use anyhow::Result;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

use crate::cmd;
use crate::store::Store;
use crate::types::{TaskId, TaskStatus};

use super::show_helpers::load_task;
use super::{diff_stat, output_text_for_task, parse_diff_stat};

/// Serialize task as JSON with events and metrics.
pub(super) fn task_json(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = load_task(store, task_id)?;
    let events = store.get_events(task_id)?;
    let event_list: Vec<serde_json::Value> = events
        .iter()
        .map(|e| {
            serde_json::json!({
                "timestamp": e.timestamp.to_rfc3339(),
                "type": e.event_kind.as_str(),
                "detail": e.detail,
                "metadata": e.metadata,
            })
        })
        .collect();
    let diff_entries = task
        .worktree_path
        .as_deref()
        .filter(|path| Path::new(path).exists())
        .map(|path| parse_diff_stat(&diff_stat(path, task.start_sha.as_deref())))
        .unwrap_or_default();
    let files_changed = diff_entries.len();
    let (insertions, deletions) =
        diff_entries
            .iter()
            .fold((0u64, 0u64), |(ins, del), entry| {
                (
                    ins + entry["insertions"].as_u64().unwrap_or(0),
                    del + entry["deletions"].as_u64().unwrap_or(0),
                )
            });
    let output = output_text_for_task(store.as_ref(), task_id, true).ok();
    let checklist_status = cmd::show_checklist::render_checklist_status(store.as_ref(), &task);
    let payload = serde_json::json!({
        "id": task.id.as_str(),
        "agent": task.agent_display_name(),
        "custom_agent": task.custom_agent_name,
        "status": task.status.as_str(),
        "prompt": task.prompt,
        "model": task.model,
        "tokens": task.tokens,
        "prompt_tokens": task.prompt_tokens,
        "duration_ms": task.duration_ms,
        "cost_usd": task.cost_usd,
        "workgroup_id": task.workgroup_id,
        "parent_task_id": task.parent_task_id,
        "worktree_branch": task.worktree_branch,
        "worktree_path": task.worktree_path,
        "repo_path": task.repo_path,
        "output_path": task.output_path,
        "output": output,
        "checklist_status": checklist_status,
        "verify": task.verify,
        "exit_code": task.exit_code,
        "verify_status": task.verify_status.as_str(),
        "pending_reason": task.pending_reason,
        "read_only": task.read_only,
        "budget": task.budget,
        "created_at": task.created_at.to_rfc3339(),
        "completed_at": task.completed_at.map(|dt| dt.to_rfc3339()),
        "events": event_list,
        "diff_stat": diff_entries,
        "diff_summary": {
            "files_changed": files_changed,
            "insertions": insertions,
            "deletions": deletions,
        },
    });
    serde_json::to_string(&payload).map_err(Into::into)
}

pub(crate) fn task_hook_json(
    task_id: &TaskId,
    agent: &str,
    status: TaskStatus,
    prompt: &str,
    worktree: Option<&str>,
    dir: Option<&str>,
    exit_code: Option<i32>,
) -> serde_json::Value {
    json!({
        "task_id": task_id.as_str(),
        "agent": agent,
        "status": status.as_str(),
        "prompt": prompt,
        "worktree": worktree,
        "dir": dir,
        "exit_code": exit_code,
    })
}
