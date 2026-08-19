// Detached-task adoption for the background reaper.
// Exports adopt_detached_task for deliberate foreground detach convergence.
// Deps: background spec, Store, task events, and process liveness checks.

use anyhow::Result;
use chrono::Local;

use super::background_spec::BackgroundRunSpec;
use super::background_reaper;
use crate::store::Store;
use crate::types::{EventKind, TaskEvent, TaskId, TaskStatus};

/// Adopt a deliberately detached foreground task. The worker (aid CLI) exited
/// on purpose; the spec carries `detached = true`. If the agent is still alive,
/// leave the task Running so the operator can reattach. If the agent has also
/// exited, the task reached a real terminal outcome — record it as Done.
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
    // Agent has exited after detach. Preserve any uncommitted work, then
    // record a real terminal outcome. We cannot recover the agent's exit
    // code (the worker that would have captured it is gone), so record Done
    // and let the operator inspect logs/diff for the actual result.
    background_reaper::preserve_zombie_changes(store, task_id, spec)?;
    let detail = "detached task completed (agent exited after foreground detach)";
    if store.update_task_status(task_id, TaskStatus::Done)? {
        store.insert_event(&TaskEvent {
            task_id: TaskId(task_id.to_string()),
            timestamp: Local::now(),
            event_kind: EventKind::Completion,
            detail: detail.to_string(),
            metadata: None,
        })?;
        background_reaper::notify_task_completion(store, task_id)?;
        cleaned.push(task_id.to_string());
    }
    Ok(())
}
