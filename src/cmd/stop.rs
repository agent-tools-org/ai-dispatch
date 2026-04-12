// Handlers for `aid stop` and `aid kill` — graceful and forced task termination.
// Sends signals to worker processes, saves partial output, and updates task status.

use anyhow::{anyhow, bail, Result};
use chrono::Local;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::background;
use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent, TaskId, TaskStatus};

const WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(200);

pub fn stop(store: &Arc<Store>, task_id: &str) -> Result<()> { terminate(store, task_id, true, "Task stopped by user", "stopped", Some("Stopped")) }

pub fn kill(store: &Arc<Store>, task_id: &str) -> Result<()> { terminate(store, task_id, false, "Task killed by user", "killed", Some("Killed")) }

pub fn terminate_any(store: &Arc<Store>, task_id: &str) -> Result<()> { terminate(store, task_id, true, "Task stopped by user", "stopped", None) }

fn terminate(
    store: &Arc<Store>,
    task_id: &str,
    graceful: bool,
    detail: &'static str,
    preserve_label: &'static str,
    print_label: Option<&'static str>,
) -> Result<()> {
    let task = ensure_non_terminal_task(store, task_id)?;
    if matches!(task.status, TaskStatus::Running | TaskStatus::AwaitingInput) {
        if let Some(pid) = background::load_worker_pid(task_id)? {
            if graceful {
                background::kill_process(pid);
                if wait_for_exit(pid) {
                    background::sigkill_process(pid);
                }
            } else {
                background::sigkill_process(pid);
                let _ = wait_for_exit(pid);
            }
        }
        if let Some(agent_pid) = background::load_agent_pid(task_id)? {
            if graceful {
                background::kill_process(agent_pid);
            } else {
                background::sigkill_process(agent_pid);
            }
        }
        crate::sandbox::kill_container(task_id);
        preserve_worktree(task_id, &task, preserve_label);
    }
    store.update_task_status(task_id, TaskStatus::Stopped)?;
    store.insert_event(&TaskEvent {
        task_id: TaskId(task_id.to_string()),
        timestamp: Local::now(),
        event_kind: EventKind::Error,
        detail: detail.to_string(),
        metadata: None,
    })?;
    background::clear_spec(task_id)?;
    if let Some(print_label) = print_label {
        println!("{print_label} {task_id}");
    }
    Ok(())
}

fn ensure_non_terminal_task(store: &Arc<Store>, task_id: &str) -> Result<Task> {
    let task = store
        .get_task(task_id)?
        .ok_or_else(|| anyhow!("Task '{task_id}' not found"))?;
    if task.status.is_terminal() {
        bail!(
            "Task '{task_id}' is already terminal (status: {})",
            task.status.as_str()
        );
    }
    Ok(task)
}

fn wait_for_exit(pid: u32) -> bool {
    let deadline = Instant::now() + WAIT_TIMEOUT;
    while Instant::now() < deadline {
        if !background::is_process_running(pid) {
            return false;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    background::is_process_running(pid)
}

fn preserve_worktree(task_id: &str, task: &Task, action: &str) {
    if !task.read_only
        && let Some(ref path) = task.worktree_path
        && Path::new(path).exists()
        && crate::commit::has_uncommitted_changes(path).unwrap_or(false)
    {
        let _ = crate::commit::auto_commit(path, task_id, &task.prompt);
        aid_info!("[aid] Preserved uncommitted changes for {action} task {task_id}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AidHomeGuard;
    use crate::store::Store;
    use crate::types::{AgentKind, EventKind, TaskId, VerifyStatus};
    use chrono::Local;
    use std::sync::Arc;
    use tempfile::TempDir;

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
        }
    }

    fn with_store<T>(f: impl FnOnce(Arc<Store>) -> T) -> T {
        let temp = TempDir::new().unwrap();
        let _guard = AidHomeGuard::set(temp.path());
        let store = Arc::new(Store::open_memory().unwrap());
        f(store)
    }

    #[test]
    fn stop_missing_task_returns_error() {
        with_store(|store| {
            let err = stop(&store, "t-missing").unwrap_err();
            assert!(err.to_string().contains("Task 't-missing' not found"));
        });
    }

    #[test]
    fn stop_done_task_returns_error() {
        with_store(|store| {
            let task = make_task("t-done", TaskStatus::Done);
            store.insert_task(&task).unwrap();
            let err = stop(&store, "t-done").unwrap_err();
            assert!(err.to_string().contains("already terminal"));
            let reloaded = store.get_task("t-done").unwrap().unwrap();
            assert_eq!(reloaded.status, TaskStatus::Done);
        });
    }

    fn assert_termination_sets_stopped(
        action: fn(&Arc<Store>, &str) -> Result<()>,
        task_id: &str,
        status: TaskStatus,
        detail: &str,
    ) {
        with_store(|store| {
            store.insert_task(&make_task(task_id, status)).unwrap();
            action(&store, task_id).unwrap();
            let updated = store.get_task(task_id).unwrap().unwrap();
            assert_eq!(updated.status, TaskStatus::Stopped);
            let events = store.get_events(task_id).unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].detail, detail);
            assert_eq!(events[0].event_kind, EventKind::Error);
        });
    }

    #[test]
    fn stop_running_task_sets_stopped() {
        assert_termination_sets_stopped(stop, "t-aa01", TaskStatus::Running, "Task stopped by user");
    }

    #[test]
    fn stop_waiting_task_sets_stopped() {
        assert_termination_sets_stopped(stop, "t-wait", TaskStatus::Waiting, "Task stopped by user");
    }

    #[test]
    fn stop_pending_task_sets_stopped() {
        assert_termination_sets_stopped(stop, "t-pend", TaskStatus::Pending, "Task stopped by user");
    }

    #[test]
    fn kill_waiting_task_sets_stopped() {
        assert_termination_sets_stopped(kill, "t-kill", TaskStatus::Waiting, "Task killed by user");
    }

    #[test]
    fn stop_attempts_agent_cleanup_when_agent_pid_exists() {
        use crate::background::{save_spec, BackgroundRunSpec};
        use tempfile::TempDir;

        let temp = TempDir::new().unwrap();
        let _guard = crate::paths::AidHomeGuard::set(temp.path());
        crate::paths::ensure_dirs().unwrap();

        let store = Arc::new(Store::open_memory().unwrap());
        let task = make_task("t-3010", TaskStatus::Running);
        store.insert_task(&task).unwrap();
        
        let spec = BackgroundRunSpec {
            task_id: "t-3010".to_string(),
            worker_pid: Some(999999),
            agent_pid: Some(888888),
            agent_name: "codex".to_string(),
            prompt: "test".to_string(),
            dir: None,
            output: None,
            result_file: None,
            model: None,
            verify: None,
            setup: None,
            iterate: None,
            eval: None,
            eval_feedback_template: None,
            judge: None,
            max_duration_mins: None,
            idle_timeout_secs: None,
            retry: 0,
            group: None,
            skills: vec![],
            checklist: vec![],
            template: None,
            interactive: false,
            on_done: None,
            cascade: vec![],
            parent_task_id: None,
            env: None,
            env_forward: None,
            sandbox: false,
            read_only: false,
            container: None,
            link_deps: true,
        };
        save_spec(&spec).unwrap();
        
        let result = stop(&store, "t-3010");
        
        assert!(result.is_ok(), "stop should succeed even with non-existent PIDs");
        assert_eq!(
            store.get_task("t-3010").unwrap().unwrap().status,
            TaskStatus::Stopped
        );
    }

    #[test]
    fn preserve_worktree_skips_read_only_tasks() {
        let mut task = make_task("t-ro01", TaskStatus::Running);
        task.read_only = true;
        preserve_worktree("t-ro01", &task, "stopped");
    }

    #[test]
    fn preserve_worktree_attempts_commit_for_non_read_only() {
        use tempfile::TempDir;

        let temp = TempDir::new().unwrap();
        let temp_path = temp.path().to_str().unwrap().to_string();

        let mut task = make_task("t-write01", TaskStatus::Running);
        task.worktree_path = Some(temp_path.clone());
        
        preserve_worktree("t-write01", &task, "stopped");
    }
}
