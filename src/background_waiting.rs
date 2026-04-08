// Stale WAIT cleanup for orphaned batch placeholders.
// Exports cleanup_stale_waiting_tasks used by background reconciliation.
// Deps: Store, config-derived timeout, notification.

use anyhow::Result;
use chrono::Local;

use crate::store::Store;
use crate::types::{TaskFilter, TaskStatus};

pub(crate) fn cleanup_stale_waiting_tasks(
    store: &Store,
    max_wait_mins: i64,
) -> Result<Vec<String>> {
    if max_wait_mins <= 0 {
        return Ok(Vec::new());
    }
    let now = Local::now();
    let mut cleaned = Vec::new();
    for task in store.list_tasks(TaskFilter::All)? {
        if task.status != TaskStatus::Waiting {
            continue;
        }
        let elapsed_secs = (now - task.created_at).num_seconds();
        if elapsed_secs < max_wait_mins * 60 {
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
