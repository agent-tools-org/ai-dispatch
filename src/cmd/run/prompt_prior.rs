// Prior-task output and sibling-summary injection.
// Exports: resolve_context_from and collect_sibling_summaries.
// Deps: Store, completion summaries, task-owned output readers.
use crate::cmd::show::extract_messages_from_log;
use crate::cmd::summary::CompletionSummary;
use crate::store::Store;
use anyhow::Result;

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

/// Resolve --context-from task IDs: read output/diff from completed tasks.
pub(in super::super) fn resolve_context_from(
    store: &Store,
    task_ids: &[String],
    omissions: &mut Vec<String>,
) -> Result<Option<String>> {
    let mut blocks = Vec::new();
    for task_id in task_ids {
        let (label, block) = if let Some(filename) = task_id.strip_prefix("shared:") {
            ("Shared File", shared_file_block(filename))
        } else if let Some(task) = store.get_task(task_id)? {
            ("Prior Task Result", prior_task_block(&task))
        } else {
            aid_warn!("[aid] Warning: --context-from task '{task_id}' not found, skipping");
            continue;
        };
        if let Some(block) = block {
            blocks.push(crate::workgroup::bound_auxiliary(&block, label, omissions));
        }
    }
    if blocks.is_empty() {
        return Ok(None);
    }
    let context = blocks.join("\n\n");
    Ok(Some(if blocks.len() > 1 {
        crate::workgroup::bound_auxiliary(&context, "Prior/shared output", omissions)
    } else {
        context
    }))
}

fn shared_file_block(filename: &str) -> Option<String> {
    let content = std::env::var_os("AID_SHARED_DIR")
        .and_then(|dir| std::fs::read_to_string(std::path::Path::new(&dir).join(filename)).ok());
    let Some(content) = content else {
        aid_warn!("[aid] Warning: shared file '{filename}' not found, skipping");
        return None;
    };
    let sanitized = sanitize_injected_content(&content);
    Some(format!(
        "[Shared File — {filename}]\n<shared-file name=\"{filename}\">\n{}\n</shared-file>",
        sanitized.trim()
    ))
}

fn prior_task_block(task: &crate::types::Task) -> Option<String> {
    let task_id = task.id.as_str();
    let mut content = String::new();
    if let Some(absence) = crate::cmd::show::missing_owned_output_absence(task) {
        aid_warn!(
            concat!("[aid] Warning: --context-from task '{task_id}' has no task-owned output file; ",
                "not using this task's log as a substitute"), task_id = task_id
        );
        content = absence;
    } else if let Ok(text) = crate::cmd::show::read_task_output(task) {
        content = text;
    }
    if content.is_empty()
        && let Some(ref log_path) = task.log_path
    {
        content = extract_messages_from_log(
            std::path::Path::new(log_path),
            true,
            Some(task.agent_display_name()),
        )
        .or_else(|| std::fs::read_to_string(log_path).ok())
        .unwrap_or_default();
    }
    if content.is_empty() {
        aid_warn!("[aid] Warning: --context-from task '{task_id}' has no output, skipping");
        return None;
    }
    let sanitized = sanitize_injected_content(&content);
    Some(format!(
        "[Prior Task Result — {} ({}, {})]\n<prior-task-output task=\"{}\">\n{}\n</prior-task-output>",
        task_id,
        task.display_route(),
        task.status.as_str(),
        task_id,
        sanitized.trim()
    ))
}

pub(in super::super) fn collect_sibling_summaries(
    store: &Store,
    group_id: &str,
    current_task_id: &str,
) -> Result<Vec<CompletionSummary>> {
    let tasks = store.list_tasks_by_group(group_id)?;
    let mut summaries = Vec::new();
    for task in &tasks {
        if task.id.as_str() == current_task_id {
            continue;
        }
        if !task.status.is_terminal() {
            continue;
        }
        if let Some(json) = store.get_completion_summary(task.id.as_str())?
            && let Ok(summary) = serde_json::from_str::<CompletionSummary>(&json)
        {
            summaries.push(summary);
        }
    }
    Ok(summaries)
}
