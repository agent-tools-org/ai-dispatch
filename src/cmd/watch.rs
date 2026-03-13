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
pub async fn run(store: &Arc<Store>, task_id: Option<&str>, group: Option<&str>, quiet: bool) -> Result<()> {
    if quiet {
        return crate::cmd::wait::run(store, task_id).await;
    }

    loop {
        // Clear terminal
        print!("\x1b[2J\x1b[H");

        if let Some(id) = task_id {
            // Single task mode
            match store.get_task(id)? {
                Some(task) => {
                    let events = store.get_events(id)?;
                    print!("{}", render_task_detail(&task, &events));

                    // Exit when task is done
                    if task.status == crate::types::TaskStatus::Done
                        || task.status == crate::types::TaskStatus::Failed
                    {
                        println!("\nTask completed. Exiting watch.");
                        return Ok(());
                    }
                }
                None => {
                    println!("Task '{}' not found.", id);
                    return Ok(());
                }
            }
        } else {
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
                    print!("{}", render_board(&all));
                }
                return Ok(());
            }
            print!("{}", render_board(&running));
        }

        sleep(Duration::from_secs(1)).await;
    }
}
