// Agent advice, budget ranking, model tiers, and the removed-auto error.
// Exports: advise(), budget_ranked_agents(), recommend_model(), and fallback helpers.
// Deps: scoring, declared profiles, model catalog, teams, and task history.

#[path = "selection_scoring.rs"]
mod selection_scoring;
#[path = "selection_quota.rs"]
mod selection_quota;
#[path = "selection_capabilities.rs"]
mod selection_capabilities;
#[path = "selection_advice.rs"]
mod selection_advice;
#[path = "selection_fallback.rs"]
mod selection_fallback;
#[path = "explicit_model.rs"]
mod explicit_model;
pub(crate) use explicit_model::declared_budget_warning;
pub(crate) use selection_advice::{AdviceCandidate, AdviceReport, advise, caller_advice};
pub(crate) use selection_fallback::{coding_fallback_for, coding_fallback_for_prompt};
pub(crate) use selection_quota::{observed_ok, quota_from, tightest_window};
use selection_scoring::{
    BUILTIN_AGENTS, Candidate, CandidateContext, candidate_for, compare_candidates,
};
use super::classifier::{self, Complexity, TaskCategory};
use super::{detect_agents, RunOpts};
use crate::agent_config;
use crate::store::Store;
use crate::types::AgentKind;
use crate::team::TeamConfig;
use std::collections::HashMap;

pub(crate) const AGENT_CAPABILITIES: &[(AgentKind, &[(TaskCategory, i32)])] =
    selection_capabilities::AGENT_CAPABILITIES;

/// Hard-error text when callers pass `auto` or leave agent empty.
pub(crate) const AUTO_AGENT_REMOVED_MSG: &str =
    "agent 'auto' was removed; declare a task profile and use `aid advise` to choose an agent";

pub(crate) fn is_removed_auto_agent(name: &str) -> bool {
    name.trim().is_empty() || name.eq_ignore_ascii_case("auto")
}

pub(crate) fn budget_ranked_agents(
    prompt: &str,
    _opts: &RunOpts,
    store: &Store,
    team: Option<&TeamConfig>,
) -> Vec<AgentKind> {
    let normalized = prompt.trim().to_lowercase();
    let prompt_len = prompt.chars().count();
    let file_count = classifier::count_file_mentions(&normalized);
    let profile = classifier::classify(prompt, file_count, prompt_len);
    let history_map: HashMap<AgentKind, (f64, usize)> = store
        .agent_success_rates()
        .unwrap_or_default()
        .into_iter()
        .map(|(kind, rate, count)| (kind, (rate, count)))
        .collect();
    let avg_cost_map: HashMap<AgentKind, f64> = store
        .agent_avg_costs()
        .unwrap_or_default()
        .into_iter()
        .collect();
    let team_default = team.and_then(|t| t.default_agent.as_deref())
        .and_then(AgentKind::parse_str);
    let ctx = CandidateContext {
        profile: &profile,
        team,
        history_map: &history_map,
        avg_cost_map: &avg_cost_map,
        team_default,
        budget: false,
        penalize_rate_limit: true,
    };
    let mut candidates: Vec<Candidate> = BUILTIN_AGENTS
        .iter()
        .filter(|kind| !agent_config::is_agent_disabled(kind.as_str()))
        .map(|&kind| candidate_for(kind, &ctx))
        .collect();
    candidates.sort_by(|a, b| compare_candidates(a, b, true).reverse());
    candidates.into_iter().map(|c| c.kind).collect()
}

pub(crate) fn recommend_model(
    agent: &AgentKind, complexity: &Complexity, budget: bool,
) -> Option<&'static str> {
    use crate::model_catalog::{budget_model, static_models_for_agent};
    if budget { return budget_model(agent); }
    let models = static_models_for_agent(agent);
    if models.is_empty() { return None; }
    let tier = match complexity {
        Complexity::Low => "cheap", Complexity::Medium => "standard", Complexity::High => "premium",
    };
    models.iter().find(|m| m.tier == tier).or_else(|| models.first()).map(|m| m.model)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod disabled_tests;
#[cfg(test)]
#[path = "selection_score_tests.rs"]
mod selection_score_tests;
