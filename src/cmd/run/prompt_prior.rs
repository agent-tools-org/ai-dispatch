// Prior-task output and sibling-summary injection.
// Exports: resolve_context_from and collect_sibling_summaries.
// Deps: Store, completion summaries, task-owned output readers.
use anyhow::Result;
use crate::cmd::show::extract_messages_from_log;
use crate::cmd::summary::CompletionSummary;
use crate::store::Store;

pub(super) fn sanitize_injected_content(content: &str) -> String {
    let mut result = Vec::new();
    let mut inside = false;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("<aid-") && !trimmed.starts_with("</aid-") {
            inside = true;
            continue;
        }
        if trimmed.starts_with("</aid-") {
            inside = false;
            continue;
        }
        if !inside {
            result.push(line);
        }
    }
    result.join("\n")
}

pub(super) fn truncate_context_content(content: &str, max_chars: usize) -> String {
    if content.len() <= max_chars {
        return content.to_string();
    }
    let end = content.floor_char_boundary(max_chars);
    content[..end].to_string()
}

/// Resolve --context-from task IDs: read output/diff from completed tasks.
pub(in super::super) fn resolve_context_from(store: &Store, task_ids: &[String]) -> Result<Option<String>> {
    let mut blocks = Vec::new();
    for task_id in task_ids {
        if let Some(filename) = task_id.strip_prefix("shared:") {
            let Some(shared_dir) = std::env::var_os("AID_SHARED_DIR") else {
                aid_warn!("[aid] Warning: shared file '{filename}' not found, skipping");
                continue;
            };
            let path = std::path::Path::new(&shared_dir).join(filename);
            let Ok(content) = std::fs::read_to_string(&path) else {
                aid_warn!("[aid] Warning: shared file '{filename}' not found, skipping");
                continue;
            };
            let sanitized = sanitize_injected_content(&content);
            blocks.push(format!(
                "[Shared File — {filename}]\n<shared-file name=\"{filename}\">\n{}\n</shared-file>",
                sanitized.trim()
            ));
            continue;
        }
        let Some(task) = store.get_task(task_id)? else {
            aid_warn!("[aid] Warning: --context-from task '{task_id}' not found, skipping");
            continue;
        };
        let mut content = String::new();
        if let Some(absence) = crate::cmd::show::missing_owned_output_absence(&task) {
            aid_warn!(
                "[aid] Warning: --context-from task '{task_id}' has no task-owned output file; not using this task's log as a substitute"
            );
            content = absence;
        } else if let Ok(text) = crate::cmd::show::read_task_output(&task) {
            content = text;
        }
        if content.is_empty()
            && let Some(ref log_path) = task.log_path
        {
            if let Some(text) = extract_messages_from_log(std::path::Path::new(log_path), false, Some(task.agent_display_name())) {
                content = truncate_context_content(&text, 2_000);
            } else if let Ok(text) = std::fs::read_to_string(log_path) {
                let lines: Vec<&str> = text.lines().collect();
                let start = lines.len().saturating_sub(50);
                content = lines[start..].join("\n");
            }
        }
        if content.is_empty() {
            aid_warn!("[aid] Warning: --context-from task '{task_id}' has no output, skipping");
            continue;
        }
        let sanitized = sanitize_injected_content(&content);
        blocks.push(format!(
            "[Prior Task Result — {} ({}, {})]\n<prior-task-output task=\"{}\">\n{}\n</prior-task-output>",
            task_id,
            task.display_route(),
            task.status.as_str(),
            task_id,
            sanitized.trim()
        ));
    }
    if blocks.is_empty() {
        return Ok(None);
    }
    Ok(Some(blocks.join("\n\n")))
}

pub(in super::super) fn collect_sibling_summaries(
    store: &Store,
    group_id: &str,
    current_task_id: &str,
) -> Result<Vec<CompletionSummary>> {
    let tasks = store.list_tasks_by_group(group_id)?;
    let mut summaries = Vec::new();
    for task in &tasks {
        if task.id.as_str() == current_task_id { continue; }
        if !task.status.is_terminal() { continue; }
        if let Some(json) = store.get_completion_summary(task.id.as_str())?
            && let Ok(summary) = serde_json::from_str::<CompletionSummary>(&json)
        {
            summaries.push(summary);
        }
    }
    summaries.truncate(5);
    Ok(summaries)
}
