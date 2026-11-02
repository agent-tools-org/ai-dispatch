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
    if args.policy.is_some() {
        return Ok(None);
    }
    let mut child = args.clone();
    if let Some(model) = crate::agent::family_route::continuation_model(
        task.agent,
        task.custom_agent_name.as_deref(),
        task.requested_model
            .as_deref()
            .or(task.observed_model.as_deref()),
    ) {
        child.model = Some(model);
        child.model_source = crate::agent::model_validation::ModelSource::AidResolved;
        child.session_id = None;
        child.force_default_model = false;
    } else {
        if crate::agent::family_route::per_family(task.agent)
            && crate::agent::family_route::healthy_model(
                task.agent,
                task.custom_agent_name.as_deref(),
                None,
            )
            .is_some()
        {
            return Ok(None);
        }
        let Some(candidate) = advice_route::automatic_candidate(Some(store), &child) else {
            return Ok(None);
        };
        advice_route::apply_candidate(&mut child, &candidate);
    }
    aid_info!(
        "[aid] Quota exhausted for {}, continuing with {} model {}",
        task.agent_display_name(),
        child.agent_name,
        child.model.as_deref().unwrap_or("(default)")
    );
    child.parent_task_id = Some(task.id.as_str().to_string());
    inherit_cascade_target(&mut child, task)?;
    Box::pin(run(store.clone(), child)).await.map(Some)
}

#[cfg(test)]
#[path = "quota_continuation_tests.rs"]
mod tests;
