// Automatic continuation preserves advice's selected route after a quota refusal.
// Exports continue_quota; deps: captured quota sentence, Task, Store and run advice.
use super::{RunArgs, inherit_cascade_target, run};
use crate::{
    cmd::run::advice_route,
    rate_limit,
    store::Store,
    types::{Task, TaskId},
};
use anyhow::Result;
use std::sync::Arc;

pub(super) async fn continue_quota(
    store: &Arc<Store>,
    args: &RunArgs,
    task: &Task,
    message: &str,
) -> Result<Option<TaskId>> {
    let model = task
        .requested_model
        .as_deref()
        .or(args.model.as_deref())
        .or(task.observed_model.as_deref());
    rate_limit::mark_rate_limited_for_model(
        &task.agent,
        task.custom_agent_name.as_deref(),
        model,
        message,
    );
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
