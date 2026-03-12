// Handler for `aid board` — list all tasks with status summary.
// Queries store and renders text table.

use anyhow::Result;
use std::sync::Arc;

use crate::board::render_board;
use crate::store::Store;
use crate::types::TaskFilter;

pub fn run(store: &Arc<Store>, running: bool, today: bool) -> Result<()> {
    let filter = if running {
        TaskFilter::Running
    } else if today {
        TaskFilter::Today
    } else {
        TaskFilter::All
    };

    let tasks = store.list_tasks(filter)?;
    print!("{}", render_board(&tasks));
    Ok(())
}
