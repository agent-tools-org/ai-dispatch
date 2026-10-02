// RunArgs-aware advice selection and exact selected-route application.
// Exports report, automatic_candidate, apply_candidate and validate_candidate_model.
// Deps: advise, RunArgs, Store, model validation and declared profiles.

use super::{RunArgs, switch_agent};
use crate::agent::model_validation::ModelSource;
use crate::agent::selection::{AdviceCandidate, AdviceReport};
use crate::store::Store;
use crate::types::{AgentKind, DeclaredTaskProfile};
use anyhow::{Result, bail};

pub(crate) fn report(store: Option<&Store>, args: &RunArgs) -> AdviceReport {
    let declared = DeclaredTaskProfile {
        difficulty: args.declared_difficulty.unwrap_or_default(),
        budget: args.declared_budget.unwrap_or_default(),
        urgency: args.declared_urgency.unwrap_or_default(),
        rigor: args.declared_rigor.unwrap_or_default(),
    };
    crate::cmd::advise::build_report(
        store,
        &args.prompt,
        declared,
        args.kind,
        args.team.as_deref(),
        0,
        None,
    )
}

pub(crate) fn automatic_candidate(
    store: Option<&Store>,
    args: &RunArgs,
) -> Option<AdviceCandidate> {
    report(store, args)
        .candidates
        .into_iter()
        .find(|candidate| {
            candidate.agent != args.agent_name
                && candidate.agent != "claude"
                && candidate.launchable(None)
        })
}

pub(crate) fn apply_candidate(args: &mut RunArgs, candidate: &AdviceCandidate) {
    switch_agent(args, candidate.agent.clone());
    args.model = candidate.model.clone();
    args.model_source = ModelSource::AidResolved;
    args.force_default_model = candidate.model.is_none();
    args.advised_route = true;
}

pub(super) fn validate_candidate_model(args: &RunArgs, kind: AgentKind) -> Result<()> {
    if !args.advised_route {
        return Ok(());
    }
    if let Some(hold) =
        crate::rate_limit::dispatch_blocking_hold_for_model(&kind, None, args.model.as_deref())
    {
        bail!(
            "advised route '{}' is held ({hold}); refusing a different default",
            args.agent_name
        );
    }
    if let Some(model) = args.model.as_deref() {
        crate::agent::model_validation::validate_model_for_agent(
            crate::agent::get_agent(kind).as_ref(),
            model,
            ModelSource::UserSupplied,
        )
        .map_err(|error| {
            anyhow::anyhow!(
                "advised model '{model}' unavailable; refusing a different default: {error}"
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "advice_route_tests.rs"]
mod tests;
