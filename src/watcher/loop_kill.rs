// Loop-kill event detail enrichment for watcher shutdown paths.
// Exports loop_kill_detail, which best-effort reads captured stderr.
// Depends on paths, truncate_text, and TaskId.

use crate::agent::truncate::truncate_text;
use crate::paths;
use crate::types::TaskId;

pub(crate) fn loop_kill_detail(task_id: &TaskId) -> String {
    let base = "Agent appears stuck in a loop — killing process";
    let Ok(stderr_content) = std::fs::read_to_string(paths::stderr_path(task_id.as_str())) else {
        return base.to_string();
    };
    let Some(line) = stderr_content
        .lines()
        .find(|line| line.contains("apply_patch verification failed"))
    else {
        return base.to_string();
    };
    format!("{base} | {}", truncate_text(line, 200))
}
