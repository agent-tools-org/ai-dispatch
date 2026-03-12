// Handler for `aid output <task-id>` — print a task's output artifact.
// Reads the stored output_path for research or non-worktree tasks.

use anyhow::{Context, Result};
use std::sync::Arc;

use crate::store::Store;
use crate::types::Task;

pub fn run(store: &Arc<Store>, task_id: &str) -> Result<()> {
    print!("{}", output_text(store, task_id)?);
    Ok(())
}

pub fn output_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = store
        .get_task(task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task '{}' not found", task_id))?;
    read_task_output(&task)
}

pub fn read_task_output(task: &Task) -> Result<String> {
    let path = task.output_path.as_deref()
        .ok_or_else(|| anyhow::anyhow!("Task has no output file"))?;
    std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read output file {}", path))
}

#[cfg(test)]
mod tests {
    use super::read_task_output;
    use crate::types::{AgentKind, Task, TaskId, TaskStatus};
    use chrono::Local;
    use tempfile::NamedTempFile;

    #[test]
    fn reads_task_output_file() {
        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "hello\n").unwrap();
        let task = Task {
            id: TaskId("t-output".to_string()),
            agent: AgentKind::Gemini,
            prompt: "prompt".to_string(),
            status: TaskStatus::Done,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            worktree_path: None,
            worktree_branch: None,
            log_path: None,
            output_path: Some(file.path().display().to_string()),
            tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            created_at: Local::now(),
            completed_at: None,
        };

        assert_eq!(read_task_output(&task).unwrap(), "hello\n");
    }
}
