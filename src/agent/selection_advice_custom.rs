// Custom-agent advice candidates: separate capability scale from built-ins.
// Exports: custom_candidates().
// Deps: custom registry, route predicate, capability helpers, declared profile types.

use crate::agent::registry::load_custom_agents;
use super::super::selection_capabilities::{custom_category_score, custom_strength_bonus};
use super::super::selection_scoring::CandidateContext;
use crate::agent_config;
use crate::model_catalog::AGENT_MODELS;
use crate::types::{AgentKind, DeclaredTaskProfile, TaskBudget};

use super::gate::Exclusions;
use super::{CustomAdviceCandidate, ELIGIBILITY_PENALTY, NOT_INSTALLED_PENALTY};

pub(super) fn custom_candidates(
    context: &CandidateContext<'_>,
    declared: DeclaredTaskProfile,
) -> Vec<CustomAdviceCandidate> {
    let floor = declared.difficulty.capability_floor();
    let mut candidates: Vec<_> = load_custom_agents().into_values()
        .filter(|config| AgentKind::parse_str(&config.id).is_none())
        .filter(|config| !agent_config::is_agent_disabled(&config.id))
        .map(|config| {
            let category_capability = custom_category_score(&config, context.profile.category);
            let strength_bonus = custom_strength_bonus(&config, context.profile.category);
            let total = category_capability + strength_bonus;
            let team_preferred = context.team.is_some_and(|team| {
                team.preferred_agents.iter().any(|item| item.eq_ignore_ascii_case(&config.id))
            });
            let model = config.forced_model.clone();
            let blocker = crate::agent::custom_route_blocker(&config.command);
            let mut exclusions = Exclusions::default();
            if let Some(blocker) = &blocker {
                exclusions.push(blocker.code(), blocker.reason());
            }
            exclusions.floor(Some(total), floor, declared.difficulty, context.profile.category);
            exclusions.budget(custom_budget_allows(model.as_deref(), declared.budget), declared.budget);
            let eligible = exclusions.is_empty();
            let (exclusion_reason, exclusion_codes) = exclusions.into_parts();
            CustomAdviceCandidate {
                agent: config.id, installed: blocker.is_none(), eligible,
                model, category_capability, strength_bonus, team_preferred,
                exclusion_reason, exclusion_codes,
            }
        })
        .collect();
    candidates.sort_by(|left, right| {
        custom_rank(right).partial_cmp(&custom_rank(left)).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let lt = left.category_capability + left.strength_bonus;
                let rt = right.category_capability + right.strength_bonus;
                rt.cmp(&lt).then_with(|| left.agent.cmp(&right.agent))
            })
    });
    candidates
}

fn custom_rank(candidate: &CustomAdviceCandidate) -> f64 {
    let mut score = f64::from(candidate.category_capability + candidate.strength_bonus);
    if !candidate.installed { score -= NOT_INSTALLED_PENALTY; }
    if !candidate.eligible { score -= ELIGIBILITY_PENALTY; }
    score
}

fn custom_budget_allows(model: Option<&str>, budget: TaskBudget) -> bool {
    if !matches!(budget, TaskBudget::Free | TaskBudget::Cheap) { return true; }
    let Some(model) = model else { return false };
    AGENT_MODELS.iter().any(|item| item.model == model && (
        item.tier == "free" || budget == TaskBudget::Cheap && item.tier == "cheap"
    ))
}
