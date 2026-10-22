// Handler for `aid watch` — live-updating text dashboard.
// Polls store and redraws terminal while honoring wait-style exit flags.

use anyhow::Result;
use std::sync::Arc;
use tokio::time::{Duration, Instant, sleep};

use crate::store::Store;
use crate::types::{Task, TaskFilter, TaskStatus};
use crate::ui::board::{render_board, render_task_detail};

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
            if watch_single(store, &task_ids[0], exit_on_await)? {
                return Ok(());
            }
        } else if task_ids.is_empty() {
            if watch_active(store, group, exit_on_await)? {
                return Ok(());
            }
        } else if watch_multiple(store, task_ids, exit_on_await)? {
            return Ok(());
        }

        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            aid_error!("[aid] Timeout after {}s.", timeout_secs.unwrap_or_default());
            std::process::exit(124);
        }
        sleep(Duration::from_secs(1)).await;
    }
}

fn exit_on_awaiting_task(store: &Store, tasks: &[Task]) -> Result<bool> {
    let Some(task) = tasks
        .iter()
        .find(|task| task.status == TaskStatus::AwaitingInput)
    else {
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

fn watch_single(store: &Store, id: &str, exit_on_await: bool) -> Result<bool> {
    // Single task mode
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
                return Ok(true);
            }
            if exit_on_await && task.status == TaskStatus::AwaitingInput {
                print_awaiting_prompt(store, id)?;
                return Ok(true);
            }
        }
        None => {
            println!("Task '{}' not found.", id);
            return Ok(true);
        }
    }

    Ok(false)
}

fn watch_active(store: &Store, group: Option<&str>, exit_on_await: bool) -> Result<bool> {
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
        return Ok(true);
    }
    if exit_on_await && exit_on_awaiting_task(store, &running)? {
        return Ok(true);
    }
    print!("{}", render_board(&running, store)?);

    Ok(false)
}

fn watch_multiple(store: &Store, task_ids: &[String], exit_on_await: bool) -> Result<bool> {
    // Multiple specified tasks mode
    let mut tasks = Vec::new();
    for id in task_ids {
        if let Some(task) = store.get_task(id)? {
            tasks.push(task);
        }
    }
    if tasks.is_empty() {
        println!("No tasks found.");
        return Ok(true);
    }
    print!("{}", render_board(&tasks, store)?);
    if exit_on_await && exit_on_awaiting_task(store, &tasks)? {
        return Ok(true);
    }

    // Exit when all tasks are terminal
    if tasks.iter().all(|t| t.status.is_terminal()) {
        println!("\nAll tasks completed. Exiting watch.");
        return Ok(true);
    }
    Ok(false)
}
