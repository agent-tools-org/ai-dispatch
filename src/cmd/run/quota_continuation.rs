// Automatic continuation preserves advice's selected route after a quota refusal.
// Exports continue_quota; deps: recorded quota evidence, Task, Store and run advice.
use super::{RunArgs, Store, Task, TaskId, inherit_cascade_target, run};
use crate::cmd::run::advice_route;
use anyhow::Result;
use std::sync::Arc;

pub(super) async fn continue_quota(
    store: &Arc<Store>,
    args: &RunArgs,
    task: &Task,
) -> Result<Option<TaskId>> {
    if store.is_caller_owned(task.id.as_str())? {
        return Ok(None);
    }
    let mut child = args.clone();
    let Some(candidate) = advice_route::automatic_candidate(Some(store), &child) else {
        return Ok(None);
    };
    aid_info!(
        "[aid] Quota exhausted for {}, auto-cascading to {}",
        task.agent_display_name(),
        candidate.agent
    );
    advice_route::apply_candidate(&mut child, &candidate);
    child.parent_task_id = Some(task.id.as_str().to_string());
    inherit_cascade_target(&mut child, task)?;
    Box::pin(run(store.clone(), child)).await.map(Some)
}

#[cfg(test)]
#[path = "quota_continuation_tests.rs"]
mod tests;
