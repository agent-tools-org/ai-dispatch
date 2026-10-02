// RunArgs-aware advice selection and exact selected-route application.
// Exports report, automatic_candidate, apply_candidate and validate_candidate_route.
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
            candidate.kind() != AgentKind::parse_str(&args.agent_name)
                && candidate.kind() != Some(AgentKind::Claude)
                && candidate.launchable(None)
        })
}

pub(crate) fn apply_candidate(args: &mut RunArgs, candidate: &AdviceCandidate) {
    switch_agent(args, candidate.agent.clone());
    args.model = candidate.model.clone();
    args.model_source = ModelSource::Advised;
    args.force_default_model = candidate.model.is_none();
    args.declared_difficulty = Some(args.declared_difficulty.unwrap_or_default());
    args.declared_budget = Some(args.declared_budget.unwrap_or_default());
    args.declared_urgency = Some(args.declared_urgency.unwrap_or_default());
    args.declared_rigor = Some(args.declared_rigor.unwrap_or_default());
}

pub(super) fn validate_candidate_route(args: &RunArgs, kind: AgentKind) -> Result<()> {
    if args.model_source != ModelSource::Advised {
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
    Ok(())
}

#[cfg(test)]
#[path = "advice_route_tests.rs"]
mod tests;
