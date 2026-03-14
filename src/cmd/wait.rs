// Handler for `aid wait` — block until tasks finish.
// Prints status transitions, milestone progress, and per-task completion summaries.
// Deps: crate::store::Store, crate::types, crate::cost

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

use crate::cost;
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
    let mut last_status: HashMap<String, String> = HashMap::new();
    let mut last_milestone: HashMap<String, String> = HashMap::new();
    let total = task_ids.len();
    let mut completed = 0usize;

    loop {
        let mut remaining = 0usize;

        for task_id in task_ids {
            let Some(task) = store.get_task(task_id)? else {
                continue;
            };

            let status = task.status;
            let status_text = status.label().to_string();
            let status_changed = last_status.insert(task_id.clone(), status_text.clone()) != Some(status_text);

            // Print status transitions with completion summary
            if status_changed {
                if status.is_terminal() {
                    completed += 1;
                    let duration = task.duration_ms
                        .map(|ms| {
                            let secs = ms / 1000;
                            if secs < 60 { format!("{secs}s") } else { format!("{}m {:02}s", secs / 60, secs % 60) }
                        })
                        .unwrap_or_else(|| "-".to_string());
                    let tokens = task.tokens
                        .map(|t| if t >= 1_000_000 { format!("{:.1}M", t as f64 / 1_000_000.0) } else if t >= 1_000 { format!("{:.1}k", t as f64 / 1_000.0) } else { t.to_string() })
                        .unwrap_or_else(|| "-".to_string());
                    println!(
                        "[{}/{}] {} {} ({}, {}tok, {})",
                        completed, total, task_id, status.label(), duration, tokens, cost::format_cost(task.cost_usd),
                    );
                } else {
                    println!("[{}/{}] {} {}", completed, total, task_id, status.label());
                }
            }

            // Print milestone progress for running tasks
            if matches!(status, TaskStatus::Running | TaskStatus::AwaitingInput) {
                if let Some(milestone) = store.latest_milestone(task_id)? {
                    let is_new = last_milestone.insert(task_id.clone(), milestone.clone()) != Some(milestone.clone());
                    if is_new {
                        let truncated = if milestone.len() > 80 { format!("{}...", &milestone[..77]) } else { milestone };
                        println!("[progress] {} — {}", task_id, truncated);
                    }
                }
            }

            if exit_on_await && status == TaskStatus::AwaitingInput && status_changed {
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
            println!("All {} task(s) completed.", total);
            return Ok(());
        }

        sleep(Duration::from_secs(2)).await;
    }
}

fn current_running_ids(store: &Arc<Store>) -> Result<Vec<String>> {
    Ok(store
        .list_tasks(TaskFilter::Running)?
        .into_iter()
        .map(|task| task.id.to_string())
        .collect())
}
