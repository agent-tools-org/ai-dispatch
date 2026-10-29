// Exact agent/model resolution for caller-owned submissions.
// Skips budget, route holds and model-group substitution before task insertion.
use super::*;
use super::resolution_phases::resolve_agent_instance;

pub(super) fn caller_owned_setup(args: &mut RunArgs) -> Result<AgentSetup> {
    let (agent_kind, custom_agent_name, agent_display_name) = resolve_enabled_agent(args)?;
    set_git_working_directory(args, agent_kind);
    let agent = resolve_agent_instance(args, agent_kind, &custom_agent_name)?;
    let model = args
        .model
        .as_deref()
        .ok_or_else(|| crate::execution::refuse("model_required", None, 3))?;
    if !agent::model_validation::validate_model_for_agent(
        agent.as_ref(),
        model,
        agent::model_validation::ModelSource::UserSupplied,
    )
    .unwrap_or(false)
    {
        return Err(crate::execution::refuse("model_not_served", Some(model), 3));
    }
    agent::api_route::reject_container(agent.as_ref(), args.container.is_some())?;
    Ok(AgentSetup {
        agent_kind,
        custom_agent_name,
        agent_display_name,
        effective_model: args.model.clone(),
        budget_active: false,
        agent,
        substituted_from: None,
    })
}
