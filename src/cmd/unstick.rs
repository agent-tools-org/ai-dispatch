// Handler for `aid unstick` manual nudges and escalation.
// Exports run() for the CLI path and keeps behavior aligned with current PTY steering.
// Deps: crate::cmd::reply, crate::store::Store, crate::unstick helpers.

use anyhow::{Result, anyhow, bail};
use chrono::Local;

use crate::cmd::reply;
use crate::store::Store;
use crate::types::{EventKind, MessageSource, TaskEvent, TaskId, TaskStatus};

const ESCALATED_EVENT_DETAIL: &str = "Task marked stalled by aid unstick";

pub fn run(store: &Store, task_id: &str, message: Option<&str>, escalate: bool) -> Result<()> {
    let task = require_running_task(store, task_id)?;
    if escalate {
        crate::unstick::mark_task_stalled(store, task.id.as_str())?;
        store.insert_event(&TaskEvent {
            task_id: TaskId(task.id.as_str().to_string()),
            timestamp: Local::now(),
            event_kind: EventKind::Milestone,
            detail: ESCALATED_EVENT_DETAIL.to_string(),
            metadata: None,
        })?;
        println!("Unstick escalated {task_id} to stalled");
        return Ok(());
    }

    let body = message
        .map(ToOwned::to_owned)
        .unwrap_or_else(crate::unstick::default_nudge_message);
    reply::run_with_source(
        store,
        task.id.as_str(),
        Some(&body),
        None,
        true,
        30,
        MessageSource::Reply,
    )?;
    println!("Sent unstick nudge to {task_id}");
    Ok(())
}

fn require_running_task(store: &Store, task_id: &str) -> Result<crate::types::Task> {
    let task = store
        .get_task(task_id)?
        .ok_or_else(|| anyhow!("Task {task_id} not found"))?;
    if task.status != TaskStatus::Running {
        bail!("Task {task_id} is {} — can only unstick running tasks", task.status.label());
    }
    Ok(task)
}

#[cfg(test)]
mod tests {
    use chrono::Local;

    use super::{ESCALATED_EVENT_DETAIL, run};
    use crate::paths;
    use crate::store::Store;
    use crate::types::{AgentKind, EventKind, Task, TaskId, TaskStatus, VerifyStatus};

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
            delivery_assessment: None,
        }
    }

    #[test]
    fn unstick_sends_default_message_when_absent() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = paths::AidHomeGuard::set(temp.path());
        let store = Store::open_memory().unwrap();
        store.insert_task(&make_task("t-unstick", TaskStatus::Running)).unwrap();

        run(&store, "t-unstick", None, false).unwrap();

        let messages = store.list_messages_for_task("t-unstick").unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, crate::unstick::default_nudge_message());
        assert_eq!(messages[0].source, crate::types::MessageSource::Reply);
    }

    #[test]
    fn unstick_escalate_sets_stalled_and_emits_event() {
        let store = Store::open_memory().unwrap();
        store.insert_task(&make_task("t-stalled", TaskStatus::Running)).unwrap();

        run(&store, "t-stalled", None, true).unwrap();

        let task = store.get_task("t-stalled").unwrap().unwrap();
        assert_eq!(task.status, TaskStatus::Stalled);
        let events = store.get_events("t-stalled").unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_kind, EventKind::Milestone);
        assert_eq!(events[0].detail, ESCALATED_EVENT_DETAIL);
    }
}
