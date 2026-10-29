// Agent and project resolution helpers for `aid run` dispatch setup.
// Exports: AgentSetup, apply_project_defaults(), resolve_agent_setup().
// Deps: agent registry, config, project defaults, budget/rate-limit helpers.
use super::RunArgs;
use crate::agent;
use crate::config;
use crate::config::agent as agent_config;
use crate::cost::usage;
use crate::routing::rate_limit;
use crate::store::Store;
use crate::types::AgentKind;
use anyhow::Result;
use std::sync::Arc;

#[path = "dispatch_resolve_held.rs"]
mod held;
#[path = "dispatch_model_info.rs"]
mod model_info;
#[path = "dispatch_run_model.rs"]
mod run_model;
pub(super) use held::maybe_insert_held_route_event;
/// Emit the one-time GitButler setup hint as a task milestone event.
pub(super) fn insert_gitbutler_setup_hint(store: &Store, task_id: &crate::types::TaskId) {
    let _ = store.insert_event(&crate::types::TaskEvent {
        task_id: task_id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: crate::types::EventKind::Milestone,
        detail: "Hint: run `but setup` from the main repo to enable GitButler integration for future tasks."
            .to_string(),
        metadata: None,
    });
}

pub(super) struct AgentSetup {
    pub agent_kind: AgentKind,
    pub custom_agent_name: Option<String>,
    pub agent_display_name: String,
    pub effective_model: Option<String>,
    pub budget_active: bool,
    pub agent: Box<dyn agent::Agent>,
    /// `Some((original, hold))` when a held primary was replaced before dispatch.
    pub substituted_from: Option<(String, String)>,
}

#[path = "dispatch_project_defaults.rs"]
mod project_defaults;
pub(super) use project_defaults::apply_project_defaults;

pub(super) fn route_custom_name(kind: AgentKind, name: &str) -> Result<Option<String>> {
    match kind {
        AgentKind::Custom => Ok(Some(name.to_string())),
        AgentKind::Codex => agent::codex::provider::route_name(),
        _ => Ok(None),
    }
}

pub(super) fn resolve_agent_setup(
    store: &Arc<Store>,
    args: &mut RunArgs,
    project_id: Option<&str>,
) -> Result<AgentSetup> {
    agent::removed::validate_run(args)?;
    if args.policy.is_some() {
        return caller_owned::caller_owned_setup(args);
    }
    let cfg = config::load_dispatch_config()?;
    let (agent_kind, custom_agent_name, resolved_agent_name) = resolve_enabled_agent(args)?;
    set_git_working_directory(args, agent_kind);
    super::advice_route::validate_candidate_route(args, agent_kind)?;
    let (agent_kind, custom_agent_name, substituted_from) = resolve_held_route(
        store,
        args,
        agent_kind,
        custom_agent_name,
        &resolved_agent_name,
    )?;
    let custom_name = custom_agent_name.as_deref();
    let auto_budget = resolve_auto_budget(store, &cfg, project_id)?;
    let budget_active =
        !args.force_default_model && (args.budget || auto_budget || cfg.selection.budget_mode);
    let run_model = run_model::resolve(args, agent_kind, custom_name, auto_budget, &cfg.selection);
    let effective_model = run_model.model.clone().filter(|_| run_model.pinned);
    // Family-metered agents: switch groups, and say so — a silent model swap
    // is the same defect as a CLI substituting a model the caller did not ask for.
    super::advice_route::validate_candidate_route(args, agent_kind)?;
    let effective_model =
        resolve_healthy_model(args, agent_kind, &custom_agent_name, effective_model);
    let (agent_kind, custom_agent_name, effective_model, substituted_from) = resolve_model_hold(
        args,
        agent_kind,
        custom_agent_name,
        effective_model,
        substituted_from,
    )?;
    finish_agent_setup(
        args,
        agent_kind,
        custom_agent_name,
        effective_model,
        substituted_from,
        budget_active,
        run_model,
    )
}
#[cfg(test)]
#[path = "dispatch_resolve_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "dispatch_api_route_tests.rs"]
mod api_route_tests;

#[cfg(test)]
#[path = "dispatch_provider_tests.rs"]
mod provider_tests;

#[path = "dispatch_resolve/phases.rs"]
mod resolution_phases;
use resolution_phases::{
    finish_agent_setup, resolve_auto_budget, resolve_enabled_agent, resolve_healthy_model,
    resolve_held_route, resolve_model_hold, set_git_working_directory,
};

#[path = "dispatch_resolve/caller_owned.rs"]
mod caller_owned;

#[cfg(test)]
#[path = "dispatch_resolve/caller_owned_tests.rs"]
mod caller_owned_tests;
