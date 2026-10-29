// Handlers for `aid stop` and `aid stop --force` — graceful and forced task termination.
// Records stopped before signalling, then preserves output and releases task resources.

use anyhow::{anyhow, bail, Result};
use std::path::Path;
use std::sync::Arc;

use crate::runtime::background;
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
    let idle_terminal = terminate_task(store, task_id, graceful, detail, preserve_label)
        .inspect_err(|error| aid_warn!("[aid] Stop unconfirmed for {task_id}: {error}"))?;
    if idle_terminal { return already_terminal(store, task_id); }
    if let Some(label) = print_label { println!("{label} {task_id}"); }
    Ok(())
}

#[path = "stop_worker.rs"]
mod worker;

fn terminate_task(
    store: &Arc<Store>, task_id: &str, graceful: bool,
    detail: &'static str, preserve_label: &str,
) -> Result<bool> {
    let task = store.get_task(task_id)?.ok_or_else(|| anyhow!("Task '{task_id}' not found"))?;
    let terminal = task.status.is_terminal()
        && !(task.status == TaskStatus::Done && task.completed_at.is_none());
    if !terminal {
        let event = TaskEvent::new(task.id.clone(), EventKind::Completion, detail.to_string());
        store.settle_task_status(task_id, TaskStatus::Stopped, None, None, Some(&event))?;
    }
    let spec = background::load_spec_if_exists(task_id)?;
    let targets = worker::Targets::load(task_id, spec.as_ref())?;
    if graceful && targets.has_live() && task.agent == crate::types::AgentKind::Codex {
        crate::runtime::codex_app_server::wait_for_interrupt(task_id);
    }
    let signalled = targets.stop(graceful)?;
    store.settle_worker(task_id, "stopped", None)?;
    if terminal && !signalled { return Ok(true); }
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
    background::clear_spec(task_id)?;
    Ok(false)
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

pub(crate) fn stop_json(store: &Arc<Store>, id: &str, force: bool) -> Result<i32> {
    let before = store.execution_snapshot(id)?
        .ok_or_else(|| crate::execution::refuse("not_found", Some(id), 1))?;
    let was_settled = before["worker"]["state"] == "settled";
    let result = terminate_task(store, id, !force, "Task stopped by user", "stopped");
    if let Err(error) = &result {
        let pid = background::load_spec_if_exists(id).ok().flatten()
            .and_then(|spec| spec.worker_pid.or(spec.agent_pid));
        store.observe_worker_state(id, "unconfirmed", pid, Some(error.to_string()))?;
    }
    let mut receipt = store.execution_snapshot(id)?
        .ok_or_else(|| crate::execution::refuse("not_found", Some(id), 1))?;
    receipt["settled"] = serde_json::json!(receipt["worker"]["state"] == "settled");
    receipt["observed_at"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
    let (outcome, detail, code) = match result {
        Ok(_) => (
            if was_settled {
                "already_settled"
            } else {
                "stopped"
            },
            None,
            0,
        ),
        Err(error) => ("unconfirmed", Some(error.to_string()), 5),
    };
    receipt["stop"] = serde_json::json!({"result": outcome, "detail": detail});
    println!("{receipt}");
    Ok(code)
}

#[cfg(test)]
mod protocol_tests {
    use super::*;

    #[test]
    fn caller_owned_stop_json_is_idempotent_and_preserves_existing_settlement() {
        let home = tempfile::tempdir().expect("home");
        let _guard = crate::support::paths::AidHomeGuard::set(home.path());
        let store = Arc::new(Store::open_memory().expect("store"));
        let mut task = tests::make_task("t-stop-json", TaskStatus::Done);
        task.completed_at = Some(chrono::Local::now());
        store.insert_task(&task).expect("task");
        assert_eq!(stop_json(&store, task.id.as_str(), false).expect("stop"), 0);
        let before = store.execution_snapshot(task.id.as_str()).expect("snapshot");
        assert_eq!(stop_json(&store, task.id.as_str(), true).expect("already settled"), 0);
        assert_eq!(store.execution_snapshot(task.id.as_str()).expect("snapshot"), before);
        assert_eq!(store.execution_facts(task.id.as_str()).expect("facts").expect("row")
            .settlement.expect("worker").outcome.as_deref(), Some("stopped"));
    }
}

#[cfg(test)]
#[path = "stop/idle_terminal_tests.rs"]
mod idle_terminal_tests;

#[cfg(all(test, unix))]
#[path = "stop/unconfirmed_tests.rs"]
mod unconfirmed_tests;
