// Handler for `aid watch` — live-updating text dashboard.
// Polls store and redraws terminal while honoring wait-style exit flags.

use anyhow::Result;
use std::sync::Arc;
use tokio::time::{sleep, Duration, Instant};

use crate::board::{render_board, render_task_detail};
use crate::store::Store;
use crate::types::{Task, TaskFilter, TaskStatus};

/// Run the watch dashboard, refreshing every second.
pub async fn run(
    store: &Arc<Store>,
    task_ids: &[String],
    group: Option<&str>,
    exit_on_await: bool,
    timeout_secs: Option<u64>,
) -> Result<()> {
    let deadline = timeout_secs.map(|secs| Instant::now() + Duration::from_secs(secs));
    loop {
        // Clear terminal
        print!("\x1b[2J\x1b[H");

        if task_ids.len() == 1 {
            // Single task mode
            let id = &task_ids[0];
            match store.get_task(id)? {
                Some(task) => {
                    let events = store.get_events(id)?;
                    let retry_chain = if task.parent_task_id.is_some() {
                        Some(store.get_retry_chain(id)?)
                    } else {
                        None
                    };
                    print!("{}", render_task_detail(&task, &events, retry_chain));

                    // Exit when task is done
                    if task.status.is_terminal() {
                        println!("\nTask completed. Exiting watch.");
                        return Ok(());
                    }
                    if exit_on_await && task.status == TaskStatus::AwaitingInput {
                        print_awaiting_prompt(store, id)?;
                        return Ok(());
                    }
                }
                None => {
                    println!("Task '{}' not found.", id);
                    return Ok(());
                }
            }
        } else if task_ids.is_empty() {
            // All running tasks mode
            let filter = if group.is_some() {
                TaskFilter::Active
            } else {
                TaskFilter::Running
            };
            let mut running = store.list_tasks(filter)?;
            if let Some(group_id) = group {
                running.retain(|task| task.workgroup_id.as_deref() == Some(group_id));
            }
            if running.is_empty() {
                println!("No running tasks.");
                // Also show recent completed tasks
                let mut all = store.list_tasks(TaskFilter::Today)?;
                if let Some(group_id) = group {
                    all.retain(|task| task.workgroup_id.as_deref() == Some(group_id));
                }
                if !all.is_empty() {
                    println!();
                    print!("{}", render_board(&all, store)?);
                }
                return Ok(());
            }
            if exit_on_await && exit_on_awaiting_task(store, &running)? {
                return Ok(());
            }
            print!("{}", render_board(&running, store)?);
        } else {
            // Multiple specified tasks mode
            let mut tasks = Vec::new();
            for id in task_ids {
                if let Some(task) = store.get_task(id)? {
                    tasks.push(task);
                }
            }
            if tasks.is_empty() {
                println!("No tasks found.");
                return Ok(());
            }
            print!("{}", render_board(&tasks, store)?);
            if exit_on_await && exit_on_awaiting_task(store, &tasks)? {
                return Ok(());
            }

            // Exit when all tasks are terminal
            if tasks.iter().all(|t| t.status.is_terminal()) {
                println!("\nAll tasks completed. Exiting watch.");
                return Ok(());
            }
        }

        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            aid_error!("[aid] Timeout after {}s.", timeout_secs.unwrap_or_default());
            std::process::exit(124);
        }
        sleep(Duration::from_secs(1)).await;
    }
}

fn exit_on_awaiting_task(store: &Store, tasks: &[Task]) -> Result<bool> {
    let Some(task) = tasks.iter().find(|task| task.status == TaskStatus::AwaitingInput) else {
        return Ok(false);
    };
    print_awaiting_prompt(store, task.id.as_str())?;
    Ok(true)
}

fn print_awaiting_prompt(store: &Store, task_id: &str) -> Result<()> {
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
    println!("{task_id} {prompt}");
    println!("Use: aid respond {task_id} \"your answer\"");
    Ok(())
}
