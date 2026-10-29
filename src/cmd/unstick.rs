// Handler for `aid unstick` manual nudges and escalation.
// Exports run() for the CLI path and keeps behavior aligned with current PTY steering.
// Deps: crate::cmd::reply, crate::store::Store, crate::task::unstick helpers.

use anyhow::{Result, anyhow, bail};
use chrono::Local;

use crate::runtime::background;
use crate::cmd::reply::{self, InputCommand};
use crate::store::Store;
use crate::types::{EventKind, MessageSource, TaskEvent, TaskId, TaskStatus};

const ESCALATED_EVENT_DETAIL: &str = "Task marked stalled by aid unstick";

pub fn run(store: &Store, task_id: &str, message: Option<&str>, escalate: bool) -> Result<()> {
    let task = require_running_task(store, task_id)?;
    if escalate {
        crate::task::unstick::mark_task_stalled(store, task.id.as_str())?;
        store.insert_event(&TaskEvent::new(
            TaskId(task.id.as_str().to_string()),
            EventKind::Milestone,
            ESCALATED_EVENT_DETAIL.to_string(),
        ))?;
        println!("Unstick escalated {task_id} to stalled");
        return Ok(());
    }

    let worker_alive = background::load_worker_pid(task.id.as_str())?
        .is_some_and(background::is_process_running);
    let agent_alive = background::load_agent_pid(task.id.as_str())?
        .is_some_and(background::is_process_running);
    if !worker_alive && !agent_alive {
        bail!(
            "Task {task_id} worker process is dead; use `aid unstick {task_id} --escalate` \
             to mark it stalled or `aid stop {task_id}` instead of queueing a nudge"
        );
    }

    let body = message
        .map(ToOwned::to_owned)
        .unwrap_or_else(crate::task::unstick::default_nudge_message);
    reply::run_with_source(
        store,
        task.id.as_str(),
        Some(&body),
        None,
        true,
        30,
        MessageSource::Reply,
        InputCommand::Unstick,
    )?;
    println!("Queued unstick nudge for {task_id}");
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


    use super::{ESCALATED_EVENT_DETAIL, run};
    use crate::runtime::background::{self, BackgroundRunSpec};
    use crate::support::paths;
    use crate::store::Store;
    use crate::types::{AgentKind, EventKind, Task, TaskId, TaskStatus};

    fn make_task(id: &str, status: TaskStatus) -> Task {
        Task {
            agent: AgentKind::Cursor,
            prompt: "test".to_string(),
            ..crate::types::Task::test_fixture(TaskId(id.to_string()), status)
        }
    }

    fn save_process_spec(task_id: &str, worker_pid: Option<u32>, agent_pid: Option<u32>) {
        let spec = serde_json::from_value::<BackgroundRunSpec>(serde_json::json!({
            "task_id": task_id,
            "worker_pid": worker_pid,
            "agent_pid": agent_pid,
            "agent_name": "codex",
            "prompt": "test",
            "dir": null,
            "output": null,
            "model": null,
            "verify": null,
            "retry": 0,
            "group": null
        }))
        .unwrap();
        background::save_spec(&spec).unwrap();
    }

    #[test]
    fn unstick_queues_default_message_when_absent() {
        let (_temp, store) = crate::support::test_env::TestHome::new().store();
        store.insert_task(&make_task("t-unstick", TaskStatus::Running)).unwrap();
        save_process_spec("t-unstick", Some(std::process::id()), None);

        run(&store, "t-unstick", None, false).unwrap();

        let messages = store.list_messages_for_task("t-unstick").unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, crate::task::unstick::default_nudge_message());
        assert_eq!(messages[0].source, crate::types::MessageSource::Reply);
    }

    #[test]
    fn unstick_dead_worker_returns_error_without_queuing_nudge() {
        let (_temp, store) = crate::support::test_env::TestHome::new().store();
        store.insert_task(&make_task("t-dead", TaskStatus::Running)).unwrap();
        save_process_spec("t-dead", Some(999_999), Some(888_888));

        let error = run(&store, "t-dead", None, false).unwrap_err();

        assert!(error.to_string().contains("worker process is dead"));
        assert!(store.list_messages_for_task("t-dead").unwrap().is_empty());
        assert!(!paths::steer_signal_path("t-dead").exists());
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
