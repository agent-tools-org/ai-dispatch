// Handlers for `aid stop` and `aid kill` — graceful and forced task termination.
// Sends signals to worker processes, saves partial output, and updates task status.

use anyhow::{anyhow, bail, Result};
use chrono::Local;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::background;
use crate::cmd::run::capture_final_worktree_state;
use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent, TaskId, TaskStatus};

const WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(200);

pub fn stop(store: &Arc<Store>, task_id: &str) -> Result<()> { terminate(store, task_id, true, "Task stopped by user", "stopped", Some("Stopped")) }

pub fn kill(store: &Arc<Store>, task_id: &str) -> Result<()> { terminate(store, task_id, false, "Task killed by user", "killed", Some("Killed")) }

pub fn terminate_any(store: &Arc<Store>, task_id: &str) -> Result<()> { terminate(store, task_id, true, "Task stopped by user", "stopped", None) }

/// Stop the entire retry tree containing `task_id`. Resolves to the chain
/// root, enumerates root + every transitive retry descendant, then stops
/// every member still in a non-terminal state. Already-terminal members are
/// silently skipped (not an error). Issue #112.
pub fn stop_retry_tree(store: &Arc<Store>, task_id: &str, force: bool) -> Result<()> {
    let root = store
        .find_retry_root(task_id)?
        .ok_or_else(|| anyhow!("Task '{task_id}' not found"))?;
    let tree = store.get_retry_tree(root.id.as_str())?;
    let total = tree.len();
    let mut stopped = 0;
    let mut skipped = 0;
    let mut failed: Vec<String> = Vec::new();
    for task in &tree {
        if task.status.is_terminal() {
            skipped += 1;
            continue;
        }
        let outcome = if force {
            terminate(
                store,
                task.id.as_str(),
                false,
                "Task killed by user (--retry-tree)",
                "killed",
                None,
            )
        } else {
            terminate(
                store,
                task.id.as_str(),
                true,
                "Task stopped by user (--retry-tree)",
                "stopped",
                None,
            )
        };
        match outcome {
            Ok(()) => stopped += 1,
            Err(err) => failed.push(format!("{}: {err}", task.id.as_str())),
        }
    }
    let label = if force { "killed" } else { "stopped" };
    println!(
        "{label} {stopped}/{total} task(s) in retry tree of {} (skipped {skipped} already-terminal)",
        root.id.as_str()
    );
    if !failed.is_empty() {
        bail!(
            "Some tasks could not be {label}:\n  {}",
            failed.join("\n  ")
        );
    }
    Ok(())
}

fn terminate(
    store: &Arc<Store>,
    task_id: &str,
    graceful: bool,
    detail: &'static str,
    preserve_label: &'static str,
    print_label: Option<&'static str>,
) -> Result<()> {
    let task = ensure_non_terminal_task(store, task_id)?;
    if matches!(task.status, TaskStatus::Running | TaskStatus::AwaitingInput | TaskStatus::Stalled) {
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
    capture_final_worktree_state(store.as_ref(), &TaskId(task_id.to_string()))?;
    if let Some(ref path) = task.worktree_path
        && let Err(err) = crate::worktree::clear_worktree_lock(Path::new(path), task_id)
    {
        aid_warn!("[aid] Warning: failed to release worktree lock for task {task_id}: {err}");
    }
    crate::task_lifecycle::mark_stopped(store.as_ref(), task_id)?;
    store.insert_event(&TaskEvent {
        task_id: TaskId(task_id.to_string()),
        timestamp: Local::now(),
        event_kind: EventKind::Completion,
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
        match crate::commit::auto_commit(path, task_id, &task.prompt) {
            Ok(_) => aid_info!("[aid] Preserved uncommitted changes for {action} task {task_id}"),
            Err(err) => aid_warn!("[aid] Warning: failed to preserve uncommitted changes for {action} task {task_id}: {err}"),
        }
    }
}

#[cfg(test)]
#[path = "stop/final_state_tests.rs"]
mod final_state_tests;

#[cfg(test)]
#[path = "stop/tests.rs"]
mod tests;
