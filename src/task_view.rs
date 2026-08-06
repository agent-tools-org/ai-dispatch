// Command-neutral task view services for UI/API callers.
// Exports diff and output readers without CLI formatting responsibilities.
// Deps: cmd::show render helpers, Task, Store.

use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

use crate::cmd;
use crate::store::Store;
use crate::types::Task;

pub fn diff_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    cmd::show::diff_text(store, task_id)
}

pub fn read_output(task: &Task) -> String {
    if let Ok(content) = cmd::show::read_task_output(task) {
        return content;
    }
    if let Some(path) = task.log_path.as_deref()
        && let Ok(content) = std::fs::read_to_string(path)
    {
        if let Some(output) = extract_messages(Path::new(path), true, Some(task.agent_display_name())) {
            return output;
        }
        if !content.trim().is_empty() {
            return content;
        }
    }
    "No output available".to_string()
}

pub fn extract_messages(log_path: &Path, full: bool, agent_name: Option<&str>) -> Option<String> {
    cmd::show::extract_messages_from_log(log_path, full, agent_name)
}
