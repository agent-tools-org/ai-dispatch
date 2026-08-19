// Detached-task adoption for the background reaper.
// Exports adopt_detached_task for deliberate foreground detach convergence.
// Deps: background spec, Store, task events, and process liveness checks.

use anyhow::Result;
use chrono::Local;

use super::background_spec::BackgroundRunSpec;
use super::background_reaper;
use crate::store::Store;
use crate::types::{EventKind, TaskEvent, TaskId, TaskStatus, VerifyStatus};

/// Adopt a deliberately detached foreground task. The worker (aid CLI) exited
/// on purpose; the spec carries `detached = true`. If the agent is still alive,
/// leave the task Running so the operator can reattach. If the agent has also
/// exited, the task reached a terminal lifecycle state — but we may not know
/// whether it succeeded or was killed.
///
/// If the watcher recorded a Completion event before the aid CLI was SIGTERM'd,
/// the agent completed while being observed and Done is honest. If no
/// Completion event survives, the agent exited unobserved: a kill, a crash and
/// a success are indistinguishable. In that case we set `VerifyStatus::Unobserved`
/// so the derived outcome is `Unverified(NoResult)` — not `Delivered` (success)
/// — regardless of whether the operator asked for verification.
pub(super) fn adopt_detached_task(
    store: &Store,
    task_id: &str,
    spec: &BackgroundRunSpec,
    cleaned: &mut Vec<String>,
) -> Result<()> {
    let agent_alive = spec.agent_pid.is_some_and(crate::background::is_process_running);
    if agent_alive {
        return Ok(());
    }
    // Agent has exited after detach. Preserve any uncommitted work.
    background_reaper::preserve_zombie_changes(store, task_id, spec)?;

    // Check for a surviving completion signal: did the watcher record a
    // Completion event before the aid CLI was SIGTERM'd? If so, the agent
    // completed while being observed and Done is honest. If not, the agent
    // exited unobserved and the result is genuinely unknown.
    let has_completion = store
        .get_events(task_id)?
        .iter()
        .any(|e| e.event_kind == EventKind::Completion);

    let (verify_status, detail) = if has_completion {
        (
            VerifyStatus::Pending,
            "detached task completed (agent exited after foreground detach; completion observed)",
        )
    } else {
        (
            VerifyStatus::Unobserved,
            "detached task adopted (agent exited unobserved after foreground detach)",
        )
    };

    store.update_verify_status(task_id, verify_status)?;
    if store.update_task_status(task_id, TaskStatus::Done)? {
        // Milestone, not Completion: Completion is the watcher's "agent finished
        // while observed" signal. Recording Completion here would make a later
        // adoption treat an unobserved exit as observed.
        store.insert_event(&TaskEvent {
            task_id: TaskId(task_id.to_string()),
            timestamp: Local::now(),
            event_kind: EventKind::Milestone,
            detail: detail.to_string(),
            metadata: None,
        })?;
        background_reaper::notify_task_completion(store, task_id)?;
        cleaned.push(task_id.to_string());
    }
    Ok(())
}
