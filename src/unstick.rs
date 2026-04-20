// Shared helpers for manual and automatic unstick flows.
// Exports: default_nudge_message, mark_task_stalled, and queue_auto_nudge.
// Deps: crate::pty_watch_idle, crate::store::Store, crate::types.

use anyhow::Result;

use crate::pty_watch_idle;
use crate::store::Store;
use crate::types::{MessageDirection, MessageSource, TaskStatus};

pub fn default_nudge_message() -> String {
    pty_watch_idle::default_nudge_message()
}

pub fn queue_auto_nudge(store: &Store, task_id: &str, message: &str) -> Result<()> {
    store.insert_message(
        task_id,
        MessageDirection::In,
        message,
        MessageSource::UnstickAuto,
    )?;
    Ok(())
}

pub fn mark_task_stalled(store: &Store, task_id: &str) -> Result<bool> {
    let Some(task) = store.get_task(task_id)? else {
        return Ok(false);
    };
    if task.status != TaskStatus::Running {
        return Ok(false);
    }
    store.update_task_status(task_id, TaskStatus::Stalled)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use chrono::Local;

    use super::*;
    use crate::store::Store;
    use crate::types::{AgentKind, Task, TaskId, VerifyStatus};

    fn make_task(id: &str, status: TaskStatus) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "prompt".to_string(),
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
            delivery_assessment: None,
        }
    }

    #[test]
    fn helper_marks_running_task_stalled() {
        let store = Store::open_memory().unwrap();
        store.insert_task(&make_task("t-stalled", TaskStatus::Running)).unwrap();

        assert!(mark_task_stalled(&store, "t-stalled").unwrap());
        assert_eq!(
            store.get_task("t-stalled").unwrap().unwrap().status,
            TaskStatus::Stalled
        );
    }

    #[test]
    fn helper_queues_auto_nudge_message() {
        let store = Store::open_memory().unwrap();
        store.insert_task(&make_task("t-nudge", TaskStatus::Running)).unwrap();

        queue_auto_nudge(&store, "t-nudge", "hello").unwrap();

        let messages = store.list_messages_for_task("t-nudge").unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].source, MessageSource::UnstickAuto);
    }
}
