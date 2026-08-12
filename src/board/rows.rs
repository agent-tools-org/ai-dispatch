// Board row formatting with project grouping.
// Exports: append_grouped_task_rows.
// Deps: project display helpers and board column helpers.

use std::collections::HashMap;

use crate::cost;
use crate::session;
use crate::store::Store;
use crate::types::{Task, TaskStatus};

use super::{
    format_duration, format_running_duration, format_tokens, format_with_outcome, short_group,
    short_parent, short_repo, task_status, truncate,
};

/// Sort by project, emit section headers, and append one board row per task.
pub(super) fn append_grouped_task_rows(
    out: &mut String,
    tasks: &[Task],
    store: &Store,
    show_repo: bool,
    awaiting_reasons: &HashMap<String, String>,
    latest_milestones: &HashMap<String, String>,
    latest_errors: &HashMap<String, String>,
) {
    let mut ordered: Vec<&Task> = tasks.iter().collect();
    ordered.sort_by(|a, b| {
        project_sort_key(a.project_id.as_deref())
            .cmp(&project_sort_key(b.project_id.as_deref()))
            .then(b.created_at.cmp(&a.created_at))
    });
    let mut last_project: Option<Option<&str>> = None;
    for task in ordered {
        let project_key = task.project_id.as_deref();
        if last_project != Some(project_key) {
            out.push_str(&format!(
                "── {} ──\n",
                crate::project::project_display(project_key)
            ));
            last_project = Some(project_key);
        }
        append_task_row(
            out,
            task,
            store,
            show_repo,
            awaiting_reasons,
            latest_milestones,
            latest_errors,
        );
    }
}

fn append_task_row(
    out: &mut String,
    task: &Task,
    store: &Store,
    show_repo: bool,
    awaiting_reasons: &HashMap<String, String>,
    latest_milestones: &HashMap<String, String>,
    latest_errors: &HashMap<String, String>,
) {
    let status = if task.status == TaskStatus::AwaitingInput {
        match awaiting_reasons.get(task.id.as_str()) {
            Some(r) => truncate(&format!("AWAIT — {r}"), 30),
            None => task.status.label().to_string(),
        }
    } else {
        let error = latest_errors.get(task.id.as_str()).cloned();
        let base = task_status(
            task,
            latest_milestones.get(task.id.as_str()).cloned(),
            error,
        );
        format_with_outcome(task, base)
    };
    let skipped = task.status == TaskStatus::Skipped;
    let duration = if skipped {
        "-".to_string()
    } else {
        task.duration_ms
            .map(format_duration)
            .unwrap_or_else(|| format_running_duration(task, store))
    };
    let tokens = if skipped {
        "-".to_string()
    } else {
        task.tokens.map(format_tokens).unwrap_or_else(|| "-".to_string())
    };
    let cost_str = if skipped {
        "-".to_string()
    } else {
        cost::format_cost_label(task.cost_usd, task.agent)
    };
    let parent = short_parent(task.parent_task_id.as_deref());
    let project = truncate(crate::project::project_display(task.project_id.as_deref()), 14);
    let group = short_group(task.workgroup_id.as_deref());
    let repo = short_repo(task.repo_path.as_deref());
    let caller = session::display(task);
    let model = task.display_model().unwrap_or_else(|| "unknown".to_string());
    let route = truncate(&task.display_route(), 36);
    if show_repo {
        out.push_str(&format!(
            "{:<11} {:<36} {:<30} {:<10} {:<10} {:<8} {:<11} {:<14} {:<12} {:<20} {:<16} {}\n",
            task.id.as_str(), route, status, duration, tokens, cost_str, parent, project, group,
            repo, caller, model,
        ));
    } else {
        out.push_str(&format!(
            "{:<11} {:<36} {:<30} {:<10} {:<10} {:<8} {:<11} {:<14} {:<12} {:<16} {}\n",
            task.id.as_str(), route, status, duration, tokens, cost_str, parent, project, group,
            caller, model,
        ));
    }
}

fn project_sort_key(project_id: Option<&str>) -> (u8, &str) {
    match project_id {
        Some(id) => (0, id),
        None => (1, crate::project::UNATTRIBUTED),
    }
}
