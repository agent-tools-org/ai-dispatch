// Custom-agent advice candidates: separate capability scale from built-ins.
// Exports: custom_candidates(), shortfall().
// Deps: custom registry, capability helpers, declared profile types.

use crate::agent::registry::load_custom_agents;
use super::super::selection_capabilities::{
    custom_category_score, custom_command_installed, custom_strength_bonus,
};
use super::super::selection_scoring::CandidateContext;
use crate::agent_config;
use crate::model_catalog::AGENT_MODELS;
use crate::types::{AgentKind, DeclaredTaskProfile, TaskBudget, TaskDifficulty};

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
            let budget_ok = custom_budget_allows(model.as_deref(), declared.budget);
            let exclusion_reason = shortfall(total, floor, declared.difficulty, budget_ok, declared.budget);
            CustomAdviceCandidate {
                agent: config.id,
                installed: custom_command_installed(&config.command),
                eligible: exclusion_reason.is_none(),
                model, category_capability, strength_bonus, team_preferred, exclusion_reason,
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

pub(super) fn shortfall(
    base: i32, floor: i32, difficulty: TaskDifficulty, budget_ok: bool, budget: TaskBudget,
) -> Option<String> {
    let mut parts = Vec::new();
    if base < floor {
        parts.push(format!("base {base} < floor {floor} for {}", difficulty.label()));
    }
    if !budget_ok {
        parts.push(format!("no model for budget {}", budget.label()));
    }
    if parts.is_empty() { None } else { Some(parts.join("; ")) }
}
