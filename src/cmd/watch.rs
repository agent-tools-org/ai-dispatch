// Handler for `aid watch` — live-updating text dashboard.
// Polls store and redraws terminal every second. No TUI framework needed.

use anyhow::Result;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

use crate::board::{render_board, render_task_detail};
use crate::store::Store;
use crate::types::TaskFilter;

/// Run the watch dashboard, refreshing every second
pub async fn run(store: &Arc<Store>, task_id: Option<&str>) -> Result<()> {
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
            let running = store.list_tasks(TaskFilter::Running)?;
            if running.is_empty() {
                println!("No running tasks.");
                // Also show recent completed tasks
                let all = store.list_tasks(TaskFilter::Today)?;
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
