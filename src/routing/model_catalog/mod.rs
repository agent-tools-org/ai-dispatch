// Shared agent model catalog: per-agent model/pricing data and catalog queries.
// Exports: static and resolved model queries, pricing overrides, budget selection.
// Deps: crate::types::AgentKind, crate::support::paths, serde, data

use std::cmp::Ordering;
use crate::types::{AgentKind, TaskBudget};

mod data;
pub(crate) mod resolved;
pub use data::{AGENT_MODELS, AGENT_PROFILES, AgentModel};
pub(crate) use resolved::{
    load_pricing_overrides, merged_agent_models, models_for_agent, PricingResponse,
};
mod served;
pub(crate) use served::unrated_served_newer_than;
pub(crate) fn static_models_for_agent(agent: &AgentKind) -> Vec<&'static AgentModel> {
    AGENT_MODELS
        .iter()
        .filter(|model| model.agent == *agent)
        .collect()
}

/// Preferred catalog tiers for a declared budget, excluding unpriced `unknown`.
fn budget_preferred_tiers(budget: TaskBudget) -> &'static [&'static str] {
    match budget {
        TaskBudget::Free => &["free"],
        TaskBudget::Cheap => &["cheap", "free"],
        TaskBudget::Standard => &["standard", "cheap", "free"],
        TaskBudget::Premium => &["premium", "standard", "cheap", "free"],
    }
}

/// True when `model` sits on a preferred (priced/known) tier for `budget`.
pub fn model_on_budget_preference(kind: AgentKind, budget: TaskBudget, model: &str) -> bool {
    AGENT_MODELS
        .iter()
        .find(|entry| entry.agent == kind && entry.model == model)
        .is_some_and(|entry| budget_preferred_tiers(budget).contains(&entry.tier))
}

fn total_price(model: &AgentModel) -> f64 {
    model.input_per_m + model.output_per_m
}

/// Free/Cheap: lowest price. Standard/Premium: measured ECI; unknown ties keep catalog order.
fn better_budget_candidate(budget: TaskBudget, left: &AgentModel, right: &AgentModel) -> Ordering {
    if matches!(budget, TaskBudget::Free | TaskBudget::Cheap) {
        return total_price(left).partial_cmp(&total_price(right)).unwrap_or(Ordering::Equal);
    }
    let category = crate::agent::classifier::TaskCategory::Research;
    let left_score = crate::routing::scores::capability_score(left.agent, left.model, category);
    let right_score = crate::routing::scores::capability_score(right.agent, right.model, category);
    match (left_score, right_score) {
        (Some(left), Some(right)) => right.partial_cmp(&left).unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn pick_in_tier(kind: AgentKind, budget: TaskBudget, tier: &str) -> Option<&'static AgentModel> {
    AGENT_MODELS
        .iter()
        .filter(|m| m.agent == kind && m.tier == tier)
        .min_by(|a, b| better_budget_candidate(budget, a, b))
}

/// Free/Cheap pool preferred tiers by lowest price; Standard/Premium walk tiers.
/// `unknown` is always last resort.
pub fn model_for_task_budget(kind: AgentKind, budget: TaskBudget) -> Option<&'static str> {
    let preferred = budget_preferred_tiers(budget);
    match budget {
        TaskBudget::Free | TaskBudget::Cheap => AGENT_MODELS
            .iter()
            .filter(|m| m.agent == kind && preferred.contains(&m.tier))
            .min_by(|a, b| better_budget_candidate(budget, a, b))
            .or_else(|| pick_in_tier(kind, budget, "unknown"))
            .map(|m| m.model),
        TaskBudget::Standard | TaskBudget::Premium => preferred
            .iter()
            .copied()
            .chain(std::iter::once("unknown"))
            .find_map(|tier| pick_in_tier(kind, budget, tier))
            .map(|m| m.model),
    }
}

/// Budget-mode / smart-route model: same rule as `model_for_task_budget(..., Cheap)`.
pub fn budget_model(agent: &AgentKind) -> Option<&'static str> {
    model_for_task_budget(*agent, TaskBudget::Cheap)
}

#[cfg(test)]
#[path = "model_catalog_tests.rs"]
mod tests;
