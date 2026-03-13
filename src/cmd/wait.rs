// Handler for `aid wait` — block until tasks finish.
// Waits for one task or the current running task set and prints status transitions.

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

use crate::store::Store;
use crate::types::{TaskFilter, TaskStatus};

pub async fn run(store: &Arc<Store>, task_ids: &[String], exit_on_await: bool) -> Result<()> {
    let task_ids = if task_ids.is_empty() {
        current_running_ids(store)?
    } else {
        task_ids.to_vec()
    };

    if task_ids.is_empty() {
        println!("No running tasks.");
        return Ok(());
    }

    println!("Waiting for {} task(s): {}", task_ids.len(), task_ids.join(", "));
    wait_for_task_ids(store, &task_ids, exit_on_await).await
}

pub async fn wait_for_task_ids(store: &Arc<Store>, task_ids: &[String], exit_on_await: bool) -> Result<()> {
    let mut last_seen = HashMap::new();

    loop {
        let mut remaining = 0usize;

        for task_id in task_ids {
            let Some(task) = store.get_task(task_id)? else {
                continue;
            };

            let status = task.status;
            let status_text = status.label().to_string();
            let changed = last_seen.insert(task_id.clone(), status_text.clone()) != Some(status_text);
            if changed {
                println!("{} {}", task_id, task.status.label());
            }

            if exit_on_await && status == TaskStatus::AwaitingInput && changed {
                let events = store.get_events(task_id)?;
                let prompt = events
                    .iter()
                    .rev()
                    .find_map(|e| {
                        e.metadata
                            .as_ref()
                            .and_then(|m| m.get("awaiting_prompt"))
                            .and_then(|v| v.as_str())
                    })
                    .unwrap_or("");
                println!("{} {}", task_id, prompt);
                println!("Use: aid respond {} \"your answer\"", task_id);
                return Ok(());
            }

            if matches!(status, TaskStatus::Pending | TaskStatus::Running | TaskStatus::AwaitingInput) {
                remaining += 1;
            }
        }

        if remaining == 0 {
            println!("All tasks completed.");
            return Ok(());
        }

        sleep(Duration::from_secs(1)).await;
    }
}

fn current_running_ids(store: &Arc<Store>) -> Result<Vec<String>> {
    Ok(store
        .list_tasks(TaskFilter::Running)?
        .into_iter()
        .map(|task| task.id.to_string())
        .collect())
}
