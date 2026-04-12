// Handler for `aid steer` — inject guidance into running PTY tasks.
// Writes steer signal for the PTY monitor loop to pick up.

use anyhow::{anyhow, bail, Result};
use crate::input_signal;
use crate::store::Store;
use crate::types::TaskStatus;

pub fn run(store: &Store, task_id: &str, message: &str) -> Result<()> {
    // 1. Check task exists
    let task = store
        .get_task(task_id)?
        .ok_or_else(|| anyhow!("Task {task_id} not found"))?;
    // 2. Check task is Running or AwaitingInput
    if task.status != TaskStatus::Running && task.status != TaskStatus::AwaitingInput {
        bail!("Task {task_id} is {} — can only steer running tasks", task.status.label());
    }
    // 3. Write steer signal
    input_signal::write_steer(task_id, message)?;
    println!("Steered {task_id}: {}", message.chars().take(80).collect::<String>());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::run;
    use crate::store::Store;
    use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
    use chrono::Local;

    fn make_task(id: &str, status: TaskStatus) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "test".to_string(),
            resolved_prompt: None,
            category: None,
            status,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            start_sha: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            pending_reason: None,
            read_only: false,
            budget: false,
            audit_verdict: None,
            audit_report_path: None,
        }
    }

    #[test]
    fn steer_non_running_task_errors() {
        let store = Store::open_memory().unwrap();
        store.insert_task(&make_task("t-steer", TaskStatus::Done)).unwrap();
        let err = run(&store, "t-steer", "pivot").unwrap_err();
        assert!(
            err.to_string().contains("can only steer running tasks"),
            "unexpected error: {err}"
        );
    }
}
