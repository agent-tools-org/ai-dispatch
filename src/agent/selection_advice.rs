// Read-only declared-profile routing advice over the production scoring engine.
// Exports: AdviceReport payloads and advise() for CLI/MCP consumers.
// Deps: selection scoring/capabilities, route inventory, caller pool, history, auth markers.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::classifier::{self, Complexity, TaskCategory, TaskProfile};
use super::selection_capabilities::{capability_row, team_override_score};
use super::selection_quota::{self, CandidateQuota};
use super::selection_scoring::{
    Candidate, CandidateContext, ScoreBreakdown, compare_candidates, cost_efficiency,
    model_capability_score, model_for_task_budget, priority, score_breakdown,
};
use crate::agent::RouteBlocker;
use crate::agent_config;
use crate::auth_marker::AuthStatus;
use crate::store::Store;
use crate::team::TeamConfig;
use crate::types::{AgentKind, DeclaredTaskProfile, TaskBudget, TaskDifficulty, TaskUrgency};

#[path = "selection_advice_custom.rs"]
mod custom;
#[path = "selection_advice_gate.rs"]
mod gate;
#[path = "selection_advice_recommend.rs"]
mod recommend;
pub(crate) use gate::{CallerAdvice, caller_advice};
use gate::{Exclusions, PoolVerdict};

pub(super) const ELIGIBILITY_PENALTY: f64 = 3.0;
pub(super) const NOT_INSTALLED_PENALTY: f64 = 1_000.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AdviceReport {
    pub declared: DeclaredTaskProfile,
    pub inferred: InferredAdvice,
    pub recommended: Option<RecommendedAdvice>,
    pub candidates: Vec<AdviceCandidate>,
    pub custom_candidates: Vec<CustomAdviceCandidate>,
    pub notes: Vec<String>,
    /// The calling session's provider pool, when detected.
    #[serde(default)]
    pub caller: Option<CallerAdvice>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct InferredAdvice {
    pub kind: TaskCategory,
    pub file_mentions: usize,
    pub chars: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct RecommendedAdvice {
    pub agent: String,
    pub model: Option<String>,
    pub score: f64,
    pub est_cost_usd: Option<f64>,
    pub est_duration_secs: Option<i64>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AdviceCandidate {
    pub agent: String,
    pub installed: bool,
    pub eligible: bool,
    pub score: f64,
    pub model: Option<String>,
    pub breakdown: ScoreBreakdown,
    pub exclusion_reason: Option<String>,
    /// Stable codes for `exclusion_reason`, one per reason.
    #[serde(default)]
    pub exclusion_codes: Vec<String>,
    /// Ranked below other pools but not excluded (caller pool, model unknown).
    #[serde(default)]
    pub demotion_reason: Option<String>,
    #[serde(default)]
    pub quota: CandidateQuota,
    #[serde(default)]
    pub auth: AuthStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CustomAdviceCandidate {
    pub agent: String,
    pub installed: bool,
    pub eligible: bool,
    pub model: Option<String>,
    pub category_capability: i32,
    pub strength_bonus: i32,
    pub team_preferred: bool,
    pub exclusion_reason: Option<String>,
    #[serde(default)]
    pub exclusion_codes: Vec<String>,
}

struct RankedCandidate {
    report: AdviceCandidate,
    order: Candidate,
    pool_excluded: bool,
}

pub(crate) fn advise(
    prompt: &str,
    declared: DeclaredTaskProfile,
    kind_override: Option<TaskCategory>,
    team: Option<&TeamConfig>,
    store: Option<&Store>,
    top: usize,
    caller: Option<CallerAdvice>,
) -> AdviceReport {
    let inferred = inferred_advice(prompt, kind_override);
    let profile = TaskProfile {
        category: inferred.kind,
        complexity: complexity_for(declared.difficulty),
    };
    let (history_map, avg_cost_map, duration_map) = history_maps(store, inferred.kind);
    let team_default = team.and_then(|item| item.default_agent.as_deref())
        .and_then(AgentKind::parse_str);
    let context = CandidateContext {
        profile: &profile,
        team,
        history_map: &history_map,
        avg_cost_map: &avg_cost_map,
        team_default,
        budget: declared.budget.uses_budget_mode(),
        declared_budget: Some(declared.budget),
        penalize_rate_limit: declared.urgency != TaskUrgency::Background,
    };
    let mut ranked = builtin_candidates(&context, declared, caller.as_ref());
    ranked.sort_by(|left, right| {
        rank_tier(left).cmp(&rank_tier(right))
            .then_with(|| ranking_score(right).partial_cmp(&ranking_score(left))
                .unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| compare_candidates(&left.order, &right.order, context.budget).reverse())
    });
    let recommended = recommend::recommendation(
        &ranked, &avg_cost_map, &duration_map, inferred.kind, declared,
    );
    let notes = recommend::availability_notes(&ranked, declared.urgency, recommended.as_ref());
    let mut candidates: Vec<_> = ranked.into_iter().map(|item| item.report).collect();
    let mut custom_candidates = custom::custom_candidates(&context, declared);
    if top > 0 {
        candidates.truncate(top);
        custom_candidates.truncate(top);
    }
    AdviceReport { declared, inferred, recommended, candidates, custom_candidates, notes, caller }
}

/// Eligible first, then eligible-but-demoted (caller's pool), then the rest.
fn rank_tier(candidate: &RankedCandidate) -> u8 {
    match (candidate.report.eligible, candidate.report.demotion_reason.is_some()) {
        (true, false) => 0,
        (true, true) => 1,
        (false, _) => 2,
    }
}

fn ranking_score(candidate: &RankedCandidate) -> f64 {
    let mut score = candidate.report.score;
    if !candidate.report.installed { score -= NOT_INSTALLED_PENALTY; }
    if !candidate.report.eligible { score -= ELIGIBILITY_PENALTY; }
    score
}

fn inferred_advice(prompt: &str, kind_override: Option<TaskCategory>) -> InferredAdvice {
    let normalized = prompt.trim().to_lowercase();
    let chars = prompt.chars().count();
    let file_mentions = classifier::count_file_mentions(&normalized);
    let kind = kind_override
        .unwrap_or_else(|| classifier::classify(prompt, file_mentions, chars).category);
    InferredAdvice { kind, file_mentions, chars }
}

fn complexity_for(difficulty: TaskDifficulty) -> Complexity {
    match difficulty {
        TaskDifficulty::Trivial | TaskDifficulty::Simple => Complexity::Low,
        TaskDifficulty::Moderate => Complexity::Medium,
        TaskDifficulty::Complex => Complexity::High,
    }
}

type HistoryMaps = (
    HashMap<AgentKind, (f64, usize)>,
    HashMap<AgentKind, f64>,
    HashMap<AgentKind, i64>,
);

fn history_maps(store: Option<&Store>, kind: TaskCategory) -> HistoryMaps {
    let Some(store) = store else { return Default::default() };
    let mut success: HashMap<_, _> = store.agent_success_rates().unwrap_or_default()
        .into_iter().map(|(agent, rate, count)| (agent, (rate, count))).collect();
    for (agent, rate, count) in store.agent_success_rates_by_category(kind.label()).unwrap_or_default() {
        success.insert(agent, (rate, count));
    }
    let costs = store.agent_avg_costs().unwrap_or_default().into_iter().collect();
    let durations = store.agent_avg_durations().unwrap_or_default().into_iter().collect();
    (success, costs, durations)
}

fn builtin_candidates(
    context: &CandidateContext<'_>,
    declared: DeclaredTaskProfile,
    caller: Option<&CallerAdvice>,
) -> Vec<RankedCandidate> {
    crate::agent::route_inventory().into_iter()
        .filter(|(kind, _)| !agent_config::is_agent_disabled(kind.as_str()))
        .map(|(kind, blocker)| builtin_candidate(context, declared, caller, kind, blocker))
        .collect()
}

fn builtin_candidate(
    context: &CandidateContext<'_>, declared: DeclaredTaskProfile,
    caller: Option<&CallerAdvice>, kind: AgentKind, blocker: Option<RouteBlocker>,
) -> RankedCandidate {
    let breakdown = score_breakdown(context, kind);
    let model = model_for_task_budget(kind, declared.budget).map(str::to_string);
    let mut exclusions = Exclusions::default();
    if let Some(blocker) = &blocker {
        exclusions.push(blocker.code(), blocker.reason());
    }
    exclusions.floor(
        team_base(context, kind), declared.difficulty.capability_floor(),
        declared.difficulty, context.profile.category,
    );
    exclusions.budget(budget_allows(declared.budget, model.as_deref()), declared.budget);
    let auth = crate::auth_marker::auth_status(kind, None);
    if auth.failed() {
        let at = auth.observed_at.as_deref().unwrap_or("unknown time");
        exclusions.push("auth_failed", format!("auth failed (observed {at})"));
    }
    let capability = model.as_deref().and_then(|name| model_capability_score(kind, name));
    let verdict = gate::pool_verdict(caller, kind, capability);
    if verdict == PoolVerdict::Weaker {
        exclusions.push("weaker_on_caller_pool", gate::WEAKER_ON_CALLER_POOL.to_string());
    }
    let demotion_reason = caller.filter(|_| verdict == PoolVerdict::Demote).map(gate::demotion_reason);
    let avg_cost = context.avg_cost_map.get(&kind).copied().unwrap_or(0.0);
    let order = Candidate {
        kind, score: breakdown.total, efficiency: cost_efficiency(breakdown.total, avg_cost),
        is_default: context.team_default == Some(kind), priority: priority(kind),
    };
    let eligible = exclusions.is_empty();
    let (exclusion_reason, exclusion_codes) = exclusions.into_parts();
    let report = AdviceCandidate {
        agent: kind.as_str().to_string(), installed: blocker.is_none(), eligible,
        score: breakdown.total, model, breakdown, exclusion_reason, exclusion_codes,
        demotion_reason, quota: selection_quota::candidate_quota(kind, None), auth,
    };
    RankedCandidate { report, order, pool_excluded: verdict == PoolVerdict::Weaker }
}

/// The measured base for the floor check: a team override, else the matrix
/// row. `None` means there is no capability data for this category.
fn team_base(context: &CandidateContext<'_>, kind: AgentKind) -> Option<i32> {
    context.team
        .and_then(|team| team_override_score(team, kind.as_str(), context.profile.category))
        .or_else(|| capability_row(kind, context.profile.category))
}

fn budget_allows(budget: TaskBudget, model: Option<&str>) -> bool {
    match budget {
        TaskBudget::Free | TaskBudget::Cheap => model.is_some(),
        TaskBudget::Standard | TaskBudget::Premium => true,
    }
}

#[cfg(test)]
#[path = "selection_advice_tests.rs"]
mod tests;
