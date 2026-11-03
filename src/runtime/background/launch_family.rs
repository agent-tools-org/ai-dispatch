// Family quota revalidation for background launches after waiting.
// Preserves continuation pins and checks the explicitly recorded refused pool.
use crate::{agent, cmd::run::RunArgs, store::Store};
use anyhow::Result;

pub(in crate::runtime::background) fn refresh_family_route(
    store: &Store,
    id: &str,
    args: &mut RunArgs,
    agent: &dyn agent::Agent,
) -> Result<()> {
    use agent::family_route;
    if !family_route::per_family(agent.kind()) {
        return Ok(());
    }
    let (policy, excluded) =
        args.family_route_inputs(store, agent.kind(), agent.rate_limit_name())?;
    let model = family_route::resolve(
        agent.kind(),
        agent.rate_limit_name(),
        args.model.as_deref(),
        policy,
        args.budget && !args.force_default_model,
        excluded.as_deref(),
    )?;
    if args.model != model {
        args.model = model;
        args.model_source = agent::model_validation::ModelSource::AidResolved;
        store.update_task_launch_model(id, args.model.as_deref(), &args.dispatch_args_json()?)?;
    }
    Ok(())
}
