// Stale WAIT cleanup for orphaned batch placeholders.
// Exports cleanup_stale_waiting_tasks used by background reconciliation.
// Deps: Store, config-derived timeout, notification.

use anyhow::Result;
use chrono::{DateTime, Local};
use std::collections::HashMap;

use crate::store::Store;
use crate::types::{Task, TaskFilter, TaskStatus};

pub(crate) fn cleanup_stale_waiting_tasks(
    store: &Store,
    max_wait_mins: i64,
) -> Result<Vec<String>> {
    if max_wait_mins <= 0 {
        return Ok(Vec::new());
    }
    let now = Local::now();
    let mut cleaned = Vec::new();
    let mut active_groups = HashMap::new();
    for task in store.list_tasks(TaskFilter::Active)? {
        let Some(elapsed_secs) = stale_wait_elapsed_secs(&task, now, max_wait_mins) else {
            continue;
        };
        if waiting_task_has_active_group(store, &task, &mut active_groups)? {
            continue;
        }
        let detail = format!(
            "wait timeout: no agent slot available after {}s (limit {}m)",
            elapsed_secs, max_wait_mins
        );
        if store.fail_waiting_with_reason(task.id.as_str(), &detail)? {
            if let Some(task) = store.get_task(task.id.as_str())? {
                crate::notify::notify_completion(&task);
            }
            cleaned.push(task.id.to_string());
        }
    }
    Ok(cleaned)
}

fn stale_wait_elapsed_secs(task: &Task, now: DateTime<Local>, max_wait_mins: i64) -> Option<i64> {
    if task.status != TaskStatus::Waiting {
        return None;
    }
    let elapsed_secs = (now - task.created_at).num_seconds();
    (elapsed_secs >= max_wait_mins * 60).then_some(elapsed_secs)
}

fn waiting_task_has_active_group(
    store: &Store,
    task: &Task,
    active_groups: &mut HashMap<String, bool>,
) -> Result<bool> {
    let Some(group_id) = task.workgroup_id.as_deref() else {
        return Ok(false);
    };
    if let Some(has_active_tasks) = active_groups.get(group_id) {
        return Ok(*has_active_tasks);
    }
    let has_active_tasks = store.list_tasks_by_group(group_id)?.into_iter().any(|group_task| {
        matches!(
            group_task.status,
            TaskStatus::Pending | TaskStatus::Running | TaskStatus::AwaitingInput
        )
    });
    active_groups.insert(group_id.to_string(), has_active_tasks);
    Ok(has_active_tasks)
}
