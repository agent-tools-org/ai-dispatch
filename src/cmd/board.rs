// Handler for `aid board` — list all tasks with status summary.
// Queries store and renders text table.

use anyhow::Result;
use std::sync::Arc;

use crate::board::render_board;
use crate::session;
use crate::store::Store;
use crate::types::TaskFilter;

pub fn run(
    store: &Arc<Store>,
    running: bool,
    today: bool,
    mine: bool,
    group: Option<&str>,
) -> Result<()> {
    let filter = if running {
        TaskFilter::Running
    } else if today {
        TaskFilter::Today
    } else {
        TaskFilter::All
    };

    let mut tasks = store.list_tasks(filter)?;
    if mine {
        tasks.retain(session::matches_current);
    }
    if let Some(group_id) = group {
        tasks.retain(|task| task.workgroup_id.as_deref() == Some(group_id));
    }
    print!("{}", render_board(&tasks));
    Ok(())
}
