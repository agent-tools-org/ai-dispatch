// Handler for `aid watch` — live-updating text dashboard with optional quiet mode.
// Polls store and redraws terminal every second. --quiet delegates to wait logic.

use anyhow::Result;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

use crate::board::{render_board, render_task_detail};
use crate::store::Store;
use crate::types::TaskFilter;

/// Run the watch dashboard, refreshing every second.
/// With `quiet`, delegates to wait logic (silent blocking).
pub async fn run(store: &Arc<Store>, task_ids: &[String], group: Option<&str>, quiet: bool, exit_on_await: bool) -> Result<()> {
    if quiet {
        return crate::cmd::wait::run(store, task_ids, group, exit_on_await).await;
    }

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
                }
                None => {
                    println!("Task '{}' not found.", id);
                    return Ok(());
                }
            }
        } else if task_ids.is_empty() {
            // All running tasks mode
            let mut running = store.list_tasks(TaskFilter::Running)?;
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

            // Exit when all tasks are terminal
            if tasks.iter().all(|t| t.status.is_terminal()) {
                println!("\nAll tasks completed. Exiting watch.");
                return Ok(());
            }
        }

        sleep(Duration::from_secs(1)).await;
    }
}
