// `aid run` input to the shared model resolver, plus the declared-budget warning.
// Exports: resolve(), input().
// Deps: agent::run_model, custom agent registry, SelectionConfig, RunArgs.

use super::RunArgs;
use crate::agent::custom::CustomAgentConfig;
use crate::agent::run_model::{RunModel, RunModelInput, resolve_run_model};
use crate::config::SelectionConfig;
use crate::types::AgentKind;

/// The model `aid run` launches for `args`, resolved exactly as `aid advise` does.
pub(super) fn resolve(
    args: &RunArgs, kind: AgentKind, custom_name: Option<&str>, auto_budget: bool,
    selection: &SelectionConfig,
) -> RunModel {
    let custom = custom_name.and_then(|name| crate::agent::registry::load_custom_agents().remove(name));
    let run_model = resolve_run_model(&input(args, kind, custom.as_ref(), auto_budget, selection));
    let warning = crate::agent::selection::declared_budget_warning(
        &args.agent_name, args.declared_budget, &run_model,
    );
    if let Some(msg) = warning {
        aid_warn!("{msg}");
    }
    run_model
}

pub(super) fn input<'a>(
    args: &'a RunArgs, kind: AgentKind, custom: Option<&'a CustomAgentConfig>, auto_budget: bool,
    selection: &SelectionConfig,
) -> RunModelInput<'a> {
    RunModelInput {
        agent_name: &args.agent_name,
        kind,
        explicit_model: args.model.as_deref(),
        force_default: args.force_default_model,
        custom,
        budget_mode: args.budget || auto_budget || selection.budget_mode,
        declared_budget: args.declared_budget,
        declared_difficulty: args.declared_difficulty,
        smart_routing: selection.smart_routing,
    }
}

#[cfg(test)]
#[path = "dispatch_run_model_tests.rs"]
mod tests;
