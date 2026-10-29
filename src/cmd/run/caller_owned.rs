// Caller-owned admission and atomic task-ID claims.
// Policy is inserted with the task row; duplicates return without dispatching.
use anyhow::Result;
use crate::{
    store::{Store, execution_protocol::ExecutionPolicy},
    types::{Task, TaskId, DispatchOrigin},
};
use super::RunArgs;

#[derive(Debug)]
pub(crate) struct Existing(pub TaskId);
impl std::fmt::Display for Existing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "existing task {}", self.0)
    }
}
impl std::error::Error for Existing {}

pub(crate) fn existing(store: &Store, args: &RunArgs) -> Result<Option<TaskId>> {
    if args.policy.is_none() {
        return Ok(None);
    }
    let Some(id) = args.existing_task_id.as_ref() else {
        return Ok(None);
    };
    let Some(task) = store.get_task(id.as_str())? else {
        return Ok(None);
    };
    let facts = store
        .execution_facts(id.as_str())?
        .ok_or_else(|| anyhow::anyhow!("Missing execution facts"))?;
    if facts.policy.as_ref().is_some_and(|policy| {
        policy.name == "caller-owned"
            && policy.version == 1
            && normalized_agent(&policy.agent) == normalized_agent(&args.agent_name)
            && Some(&policy.model) == args.model.as_ref()
    }) && normalized_agent(task.agent_display_name()) == normalized_agent(&args.agent_name)
        && task.requested_model == args.model
    {
        return Ok(Some(id.clone()));
    }
    Err(crate::execution::refuse("id_conflict", Some(id.as_str()), 4))
}

pub(super) fn claim(store: &Store, task: &Task, args: &RunArgs) -> Result<()> {
    let policy = ExecutionPolicy {
        name: "caller-owned".into(),
        version: 1,
        agent: normalized_agent(&args.agent_name),
        model: args
            .model
            .clone()
            .ok_or_else(|| crate::execution::refuse("model_required", None, 3))?,
        accepted_at: chrono::Utc::now().to_rfc3339(),
    };
    match store.insert_task_with_policy(task, DispatchOrigin::Automatic, Some(&policy)) {
        Ok(()) => Ok(()),
        Err(error) => {
            if let Some(id) = existing(store, args)? {
                return Err(Existing(id).into());
            }
            Err(error)
        }
    }
}

pub(super) fn emit(store: &Store, args: &RunArgs, id: &TaskId, submission: &str) -> Result<()> {
    if args.execution_json && args.policy.is_some() {
        let mut receipt = crate::cmd::execution_receipt::receipt(store, id.as_str())?;
        receipt["submission"] = serde_json::json!(submission);
        println!("{receipt}");
    }
    Ok(())
}

pub(super) fn preparation_error(store: &Store, args: &RunArgs, error: anyhow::Error) -> Result<TaskId> {
    if let Some(existing) = error.downcast_ref::<Existing>() {
        emit(store, args, &existing.0, "existing")?;
        return Ok(existing.0.clone());
    }
    if let Some(id) = args.existing_task_id.as_ref() {
        if args.policy.is_none() {
            let _ = crate::task::lifecycle::fail_waiting_with_reason(store, id.as_str(), &error.to_string());
        } else if error.downcast_ref::<crate::execution::Refusal>().is_none()
            && store.execution_facts(id.as_str())?.is_some_and(|facts| {
                facts.policy.is_some()
                    && facts
                        .settlement
                        .is_some_and(|worker| worker.outcome.as_deref() == Some("launch_failed"))
            })
        {
            emit(store, args, id, "launched")?;
            return Ok(id.clone());
        }
    }
    Err(error)
}


fn normalized_agent(name: &str) -> String {
    crate::types::AgentKind::parse_str(name)
        .map(|kind| kind.as_str().to_owned())
        .unwrap_or_else(|| name.to_lowercase())
}

#[cfg(test)]
#[path = "caller_owned_admission_tests.rs"]
mod admission_tests;
