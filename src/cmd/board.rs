// Handler for `aid board` — list all tasks with status summary.
// Detects repeated calls with no status changes and warns callers.
// Deps: crate::store, crate::board, crate::background, crate::session

use anyhow::Result;
use std::sync::Arc;

use crate::background;
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

    background::check_zombie_tasks(store)?;
    let mut tasks = store.list_tasks(filter)?;
    if mine {
        tasks.retain(session::matches_current);
    }
    if let Some(group_id) = group {
        tasks.retain(|task| task.workgroup_id.as_deref() == Some(group_id));
    }

    // Detect repeated calls with no changes — warn caller to use watch instead
    let fingerprint = task_fingerprint(&tasks);
    let marker_path = crate::paths::aid_dir().join("board-last.txt");
    if let Ok(prev) = std::fs::read_to_string(&marker_path) {
        if prev.trim() == fingerprint {
            eprintln!("[aid] No status changes since last check. Use `aid watch --quiet` for automatic notification instead of polling.");
        }
    }
    let _ = std::fs::write(&marker_path, &fingerprint);

    print!("{}", render_board(&tasks, store)?);
    Ok(())
}

/// Compact fingerprint of task statuses for change detection.
fn task_fingerprint(tasks: &[crate::types::Task]) -> String {
    let mut parts: Vec<String> = tasks
        .iter()
        .map(|t| format!("{}:{}", t.id, t.status.label()))
        .collect();
    parts.sort();
    parts.join(",")
}
