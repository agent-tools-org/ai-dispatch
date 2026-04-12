// JSONL event stream for `aid watch --stream`.
// Exports run() and testable event serialization helpers.
// Deps: Store task queries, background cleanup, serde_json.

use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep, Duration, Instant};

use crate::store::Store;
use crate::types::{Task, TaskFilter, TaskStatus};

const POLL_INTERVAL: Duration = Duration::from_secs(2);

pub async fn run(
    store: &Arc<Store>,
    task_ids: &[String],
    group: Option<&str>,
    timeout_secs: Option<u64>,
) -> Result<()> {
    let deadline = timeout_secs.map(|secs| Instant::now() + Duration::from_secs(secs));
    let tracked_group = if task_ids.is_empty() { group } else { None };
    let mut state = StreamState::new(task_ids.to_vec());
    loop {
        let _ = crate::background::check_zombie_tasks(store);
        state.add_group_tasks(store, tracked_group)?;
        if state.task_ids.is_empty() {
            println!("{}", json!({ "event": "watch_empty" }));
            return Ok(());
        }
        let tasks = state.load_tasks(store)?;
        if tasks.is_empty() {
            println!("{}", json!({ "event": "watch_empty" }));
            return Ok(());
        }
        state.emit_changes(store, &tasks)?;
        if tasks.iter().all(|task| task.status.is_terminal()) {
            println!("{}", summary_event("batch_complete", &tasks));
            return Ok(());
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            println!("{}", summary_event("watch_timeout", &tasks));
            return Ok(());
        }
        sleep(POLL_INTERVAL).await;
    }
}

struct StreamState {
    task_ids: Vec<String>,
    last_status: HashMap<String, TaskStatus>,
}

impl StreamState {
    fn new(task_ids: Vec<String>) -> Self {
        Self {
            task_ids,
            last_status: HashMap::new(),
        }
    }

    fn add_group_tasks(&mut self, store: &Store, group: Option<&str>) -> Result<()> {
        let Some(group_id) = group else {
            return Ok(());
        };
        for task in group_active_tasks(store, group_id)? {
            let task_id = task.id.to_string();
            if !self.task_ids.contains(&task_id) {
                self.task_ids.push(task_id);
            }
        }
        Ok(())
    }

    fn load_tasks(&self, store: &Store) -> Result<Vec<Task>> {
        let mut tasks = Vec::new();
        for task_id in &self.task_ids {
            if let Some(task) = store.get_task(task_id)? {
                tasks.push(task);
            }
        }
        Ok(tasks)
    }

    fn emit_changes(&mut self, store: &Store, tasks: &[Task]) -> Result<()> {
        for task in tasks {
            let task_id = task.id.as_str();
            let changed = self.last_status.get(task_id) != Some(&task.status);
            if !changed {
                continue;
            }
            self.last_status.insert(task_id.to_string(), task.status);
            println!("{}", task_event_json(store, task, tasks)?);
        }
        Ok(())
    }
}

fn group_active_tasks(store: &Store, group_id: &str) -> Result<Vec<Task>> {
    let mut tasks = store.list_tasks(TaskFilter::Active)?;
    tasks.retain(|task| task.workgroup_id.as_deref() == Some(group_id));
    Ok(tasks)
}

pub(crate) fn task_event_json(store: &Store, task: &Task, tasks: &[Task]) -> Result<Value> {
    let mut event = json!({
        "event": event_name(task.status),
        "task": task.id.as_str(),
        "agent": task.agent_display_name(),
        "status": task.status.as_str(),
        "elapsed_secs": elapsed_secs(task),
        "progress": progress(tasks),
        "remaining": remaining(tasks),
    });
    if task.status == TaskStatus::Failed
        && let Some(reason) = store.latest_error(task.id.as_str())
    {
        event["reason"] = json!(reason);
    }
    Ok(event)
}

fn summary_event(event: &str, tasks: &[Task]) -> Value {
    json!({
        "event": event,
        "progress": progress(tasks),
        "remaining": remaining(tasks),
        "failed": tasks.iter().filter(|task| task.status == TaskStatus::Failed).count(),
        "total": tasks.len(),
    })
}

fn event_name(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Done | TaskStatus::Merged => "task_done",
        TaskStatus::Failed => "task_failed",
        TaskStatus::Stopped => "task_stopped",
        TaskStatus::Skipped => "task_skipped",
        _ => "task_status",
    }
}

fn progress(tasks: &[Task]) -> String {
    let complete = tasks.iter().filter(|task| task.status.is_terminal()).count();
    format!("{complete}/{}", tasks.len())
}

fn remaining(tasks: &[Task]) -> usize {
    tasks.iter().filter(|task| !task.status.is_terminal()).count()
}

fn elapsed_secs(task: &Task) -> i64 {
    task.duration_ms
        .map(|duration_ms| duration_ms / 1_000)
        .unwrap_or_else(|| (chrono::Local::now() - task.created_at).num_seconds().max(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use crate::store::Store;
    use crate::types::{AgentKind, TaskId, VerifyStatus};

    #[test]
    fn failed_task_event_includes_reason_and_progress() {
        let store = Store::open_memory().unwrap();
        let mut failed = task("t-fail", TaskStatus::Failed);
        failed.duration_ms = Some(5_000);
        let done = task("t-done", TaskStatus::Done);
        store.insert_task(&failed).unwrap();
        store
            .insert_event(&crate::types::TaskEvent {
                task_id: failed.id.clone(),
                timestamp: Local::now(),
                event_kind: crate::types::EventKind::Error,
                detail: "agent_crash: exited 1".to_string(),
                metadata: None,
            })
            .unwrap();

        let event = task_event_json(&store, &failed, &[failed.clone(), done]).unwrap();

        assert_eq!(event["event"], "task_failed");
        assert_eq!(event["reason"], "agent_crash: exited 1");
        assert_eq!(event["progress"], "2/2");
        assert_eq!(event["remaining"], 0);
        assert_eq!(event["elapsed_secs"], 5);
    }

    fn task(id: &str, status: TaskStatus) -> Task {
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
        }
    }
}
