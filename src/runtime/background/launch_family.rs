// Family quota revalidation for background launches after waiting.
// Preserves continuation pins and checks the parent model's refused pool.
use crate::{agent, cmd::run::RunArgs, store::Store};
use anyhow::Result;

pub(in crate::runtime::background) fn refresh_family_route(
    store: &Store,
    id: &str,
    args: &mut RunArgs,
    agent: &dyn agent::Agent,
) -> Result<()> {
    use agent::family_route::{self, PinPolicy};
    if !family_route::per_family(agent.kind()) {
        return Ok(());
    }
    let failed = if args.force_default_model
        || args.model_source == agent::model_validation::ModelSource::AidResolved
    {
        args.parent_task_id
            .as_deref()
            .map(|id| store.get_task(id))
            .transpose()?
            .flatten()
            .filter(|task| {
                task.agent == agent.kind()
                    && task.custom_agent_name.as_deref() == agent.rate_limit_name()
            })
            .and_then(|task| task.requested_model.or(task.observed_model))
    } else {
        None
    };
    let policy = pin_policy(args, failed.as_deref());
    let excluded = failed
        .as_deref()
        .filter(|_| args.force_default_model || policy == PinPolicy::Continuation);
    let model = family_route::resolve(
        agent.kind(),
        agent.rate_limit_name(),
        args.model.as_deref(),
        policy,
        args.budget && !args.force_default_model,
        excluded,
    )?;
    if args.model != model {
        args.model = model;
        args.model_source = agent::model_validation::ModelSource::AidResolved;
        store.update_task_launch_model(id, args.model.as_deref(), &args.dispatch_args_json()?)?;
    }
    Ok(())
}

fn pin_policy(args: &RunArgs, parent_model: Option<&str>) -> agent::family_route::PinPolicy {
    use agent::family_route::PinPolicy;
    if args.policy.is_some() || args.model_source == agent::model_validation::ModelSource::Advised {
        PinPolicy::Exact
    } else if args.parent_task_id.is_some()
        && args.model.is_some()
        && args.model.as_deref() != parent_model
        && args.model_source == agent::model_validation::ModelSource::AidResolved
    {
        PinPolicy::Continuation
    } else if args.model_source == agent::model_validation::ModelSource::UserSupplied {
        PinPolicy::WarnOnReplacement
    } else {
        PinPolicy::Automatic
    }
}
