// Handlers for `aid stop` and `aid stop --force` — graceful and forced task termination.
// Records stopped before signalling, then preserves output and releases task resources.

use anyhow::{anyhow, bail, Result};
use std::path::Path;
use std::sync::Arc;

use crate::runtime::background;
use crate::runtime::execution_close::{close_execution, Closed, Closer};
use crate::cmd::run::capture_final_worktree_state;
use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent, TaskId, TaskStatus};

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
        if task.status.is_terminal() && !(task.status == TaskStatus::Done && task.completed_at.is_none()) {
            skipped += 1;
            continue;
        }
        let (detail, label) = if force {
            ("Task killed by user (--retry-tree)", "killed")
        } else {
            ("Task stopped by user (--retry-tree)", "stopped")
        };
        let outcome = terminate(store, task.id.as_str(), !force, detail, label, None);
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
    let task = store.get_task(task_id)?.ok_or_else(|| anyhow!("Task '{task_id}' not found"))?;
    if task.status.is_terminal() && !(task.status == TaskStatus::Done && task.completed_at.is_none())
        && !store.is_caller_owned(task_id)? {
        return already_terminal(store, task_id);
    }
    let (idle_terminal, _) = terminate_task(store, task_id, graceful, detail, preserve_label, false)
        .inspect_err(|error| aid_warn!("[aid] Stop unconfirmed for {task_id}: {error}"))?;
    if idle_terminal && !store.is_caller_owned(task_id)? { return already_terminal(store, task_id); }
    if let Some(label) = print_label { println!("{label} {task_id}"); }
    Ok(())
}

#[path = "stop_worker.rs"]
mod worker;

fn terminate_task(
    store: &Arc<Store>, task_id: &str, graceful: bool,
    detail: &'static str, preserve_label: &str, json: bool,
) -> Result<(bool, Closed)> {
    let task = store.get_task(task_id)?.ok_or_else(|| anyhow!("Task '{task_id}' not found"))?;
    let terminal = task.status.is_terminal()
        && !(task.status == TaskStatus::Done && task.completed_at.is_none());
    if terminal && !json && !store.is_caller_owned(task_id)? {
        already_terminal(store, task_id)?;
    }
    if !terminal {
        let event = TaskEvent::new(task.id.clone(), EventKind::Completion, detail.to_string());
        store.settle_task_status(task_id, TaskStatus::Stopped, None, None, Some(&event))?;
    }
    let mut spec = background::load_spec_if_exists(task_id)?;
    let targets = worker::Targets::load(task_id, spec.as_ref())?;
    if graceful && targets.has_live() && task.agent == crate::types::AgentKind::Codex {
        crate::runtime::codex_app_server::wait_for_interrupt(task_id);
    }
    let mut signalled = false;
    if let Some(spec) = spec.as_mut() {
        crate::runtime::execution_processes::capture(spec)?;
        signalled = background::stop_task_processes(spec.worker_pid, spec, !graceful);
    }
    signalled |= targets.stop(graceful)?;
    if let Some(spec) = &spec { background::stop_task_processes(spec.worker_pid, spec, true); }
    if terminal && !signalled && spec.as_ref().is_none_or(|s| s.tool_sessions.is_empty() && s.container_name.is_none()) {
        let closed = close_execution(store, task_id, "stopped", Closer::Stop, spec.as_ref())?;
        return closed_stop(true, closed);
    }
    finish_stop(store, task_id, &task, preserve_label, signalled, spec.as_ref())
}

fn finish_stop(
    store: &Arc<Store>, task_id: &str, task: &Task, preserve_label: &str,
    signalled: bool, spec: Option<&background::BackgroundRunSpec>,
) -> Result<(bool, Closed)> {
    if signalled {
        crate::runtime::sandbox::kill_container(task_id);
        preserve_worktree(task_id, &task, preserve_label);
    }
    capture_final_worktree_state(store.as_ref(), &TaskId(task_id.to_string()))?;
    if let Some(ref path) = task.worktree_path
        && let Err(error) = crate::workspace::worktree::clear_worktree_lock(Path::new(path), task_id)
    {
        aid_warn!("[aid] Warning: failed to release worktree lock for task {task_id}: {error}");
    }
    if let Some(name) = spec.and_then(|s| s.container_name.as_ref()) {
        crate::runtime::container::stop_container(name);
    }
    background::clear_spec(task_id)?;
    let closed = close_execution(store, task_id, "stopped", Closer::Stop, spec)?;
    closed_stop(false, closed)
}

fn closed_stop(idle: bool, closed: Closed) -> Result<(bool, Closed)> {
    if let Closed::Unconfirmed(reason) = &closed { bail!("{reason}"); }
    Ok((idle, closed))
}

fn already_terminal(store: &Store, task_id: &str) -> Result<()> {
    let task = store.get_task(task_id)?.ok_or_else(|| anyhow!("Task '{task_id}' not found"))?;
    let reason = store.get_events(task_id)?.into_iter().rev()
        .find(|event| matches!(event.event_kind, EventKind::Completion | EventKind::Error))
        .map(|event| event.detail).unwrap_or_else(|| "no terminal reason recorded".to_string());
    bail!("task already {}: {}", task.status.as_str(), reason)
}

fn preserve_worktree(task_id: &str, task: &Task, action: &str) {
    if !task.read_only
        && let Some(ref path) = task.worktree_path
        && Path::new(path).exists()
        && crate::workspace::commit::has_uncommitted_changes(path).unwrap_or(false)
    {
        match crate::workspace::commit::auto_commit(path, task_id, &task.prompt) {
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

#[cfg(test)]
#[path = "stop/settle_first_tests.rs"]
mod settle_first_tests;

#[cfg(test)]
#[path = "stop/settle_writer_tests.rs"]
mod settle_writer_tests;

#[cfg(test)]
#[path = "stop/settle_dispatch_tests.rs"]
mod settle_dispatch_tests;

#[cfg(test)]
#[path = "stop/settle_launch_tests.rs"]
mod settle_launch_tests;

#[cfg(test)]
#[path = "stop/settle_done_tests.rs"]
mod settle_done_tests;

#[cfg(test)]
#[path = "stop/settle_worker_tests.rs"]
mod settle_worker_tests;

#[cfg(test)]
#[path = "stop/terminal_events_tests.rs"]
mod terminal_events_tests;

#[cfg(all(test, unix))]
#[path = "stop/worker_identity_tests.rs"]
mod worker_identity_tests;

#[path = "stop_json.rs"]
mod json;
pub(crate) use json::stop_json;

#[cfg(test)]
#[path = "stop/idle_terminal_tests.rs"]
mod idle_terminal_tests;

#[cfg(all(test, unix))]
#[path = "stop/unconfirmed_tests.rs"]
mod unconfirmed_tests;

#[cfg(test)]
#[path = "stop/close_tests.rs"]
mod close_tests;
