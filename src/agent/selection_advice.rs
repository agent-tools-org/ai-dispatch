// Read-only declared-profile routing advice over the production scoring engine.
// Exports: AdviceReport payloads and advise() for CLI/MCP consumers.
// Deps: selection scoring/capabilities, inventory, history store, rate markers.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::classifier::{self, Complexity, TaskCategory, TaskProfile};
use super::selection_capabilities::{base_score, team_override_score};
use super::selection_quota::{self, CandidateQuota, NoteTarget};
use super::selection_scoring::{
    Candidate, CandidateContext, ScoreBreakdown, compare_candidates, cost_efficiency,
    model_for_task_budget, priority, score_breakdown,
};
use crate::agent_config;
use crate::store::Store;
use crate::team::TeamConfig;
use crate::types::{AgentKind, DeclaredTaskProfile, TaskBudget, TaskDifficulty, TaskUrgency};

#[path = "selection_advice_custom.rs"]
mod custom;

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
    #[serde(default)]
    pub quota: CandidateQuota,
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
}

struct RankedCandidate {
    report: AdviceCandidate,
    order: Candidate,
}

pub(crate) fn advise(
    prompt: &str,
    declared: DeclaredTaskProfile,
    kind_override: Option<TaskCategory>,
    team: Option<&TeamConfig>,
    store: Option<&Store>,
    top: usize,
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
    let mut ranked = builtin_candidates(&context, declared);
    ranked.sort_by(|left, right| {
        ranking_score(right).partial_cmp(&ranking_score(left)).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| compare_candidates(&left.order, &right.order, context.budget).reverse())
    });
    let recommended = recommendation(&ranked, &avg_cost_map, &duration_map, inferred.kind, declared);
    let notes = availability_notes(&ranked, declared.urgency, recommended.as_ref());
    let mut candidates: Vec<_> = ranked.into_iter().map(|item| item.report).collect();
    let mut custom_candidates = custom::custom_candidates(&context, declared);
    if top > 0 {
        candidates.truncate(top);
        custom_candidates.truncate(top);
    }
    AdviceReport { declared, inferred, recommended, candidates, custom_candidates, notes }
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
) -> Vec<RankedCandidate> {
    let installed: HashSet<_> = super::detect_agents().into_iter().collect();
    let floor = declared.difficulty.capability_floor();
    AgentKind::ALL_BUILTIN.iter().copied().chain([AgentKind::Claude])
        .filter(|kind| !agent_config::is_agent_disabled(kind.as_str()))
        .map(|kind| {
            let breakdown = score_breakdown(context, kind);
            let model = model_for_task_budget(kind, declared.budget).map(str::to_string);
            let base = team_base(context, kind);
            let budget_ok = budget_allows(kind, declared.budget, model.as_deref());
            let exclusion_reason = custom::shortfall(
                base, floor, declared.difficulty, budget_ok, declared.budget,
            );
            ranked(
                kind.as_str().to_string(), kind, installed.contains(&kind),
                exclusion_reason.is_none(), model, breakdown, exclusion_reason, context,
            )
        })
        .collect()
}

fn team_base(context: &CandidateContext<'_>, kind: AgentKind) -> i32 {
    context.team
        .and_then(|team| team_override_score(team, kind.as_str(), context.profile.category))
        .unwrap_or_else(|| base_score(kind, context.profile.category))
}

fn budget_allows(_kind: AgentKind, budget: TaskBudget, model: Option<&str>) -> bool {
    match budget {
        TaskBudget::Free | TaskBudget::Cheap => model.is_some(),
        TaskBudget::Standard | TaskBudget::Premium => true,
    }
}

fn ranked(
    name: String, kind: AgentKind, installed: bool, eligible: bool,
    model: Option<String>, breakdown: ScoreBreakdown, exclusion_reason: Option<String>,
    context: &CandidateContext<'_>,
) -> RankedCandidate {
    let avg_cost = context.avg_cost_map.get(&kind).copied().unwrap_or(0.0);
    let order = Candidate {
        kind, score: breakdown.total, efficiency: cost_efficiency(breakdown.total, avg_cost),
        is_default: context.team_default == Some(kind), priority: priority(kind),
    };
    let quota = selection_quota::candidate_quota(kind, None);
    let report = AdviceCandidate {
        agent: name, installed, eligible, score: breakdown.total, model, breakdown,
        exclusion_reason, quota,
    };
    RankedCandidate { report, order }
}

fn recommendation(
    ranked: &[RankedCandidate], costs: &HashMap<AgentKind, f64>,
    durations: &HashMap<AgentKind, i64>, kind: TaskCategory, declared: DeclaredTaskProfile,
) -> Option<RecommendedAdvice> {
    // Ranking already applies the eligibility penalty; prefer installed, else first.
    let selected = ranked.iter().find(|item| item.report.installed).or_else(|| ranked.first())?;
    let model_suffix = selected.report.model.as_deref().map(|model| format!("/{model}")).unwrap_or_default();
    let mut reason = format!(
        "{}/{} → {}{} (score: {:.1})",
        declared.difficulty.label(), kind.label(), selected.report.agent, model_suffix,
        selected.report.score,
    );
    if let Some(clause) = quota_pick_clause(ranked, selected) {
        reason.push_str(&clause);
    }
    Some(RecommendedAdvice {
        agent: selected.report.agent.clone(), model: selected.report.model.clone(),
        score: selected.report.score, est_cost_usd: costs.get(&selected.order.kind).copied(),
        est_duration_secs: durations.get(&selected.order.kind).copied(),
        reason,
    })
}

fn quota_pick_clause(ranked: &[RankedCandidate], selected: &RankedCandidate) -> Option<String> {
    let unconstrained = ranked.iter().filter(|item| item.report.installed).max_by(|left, right| {
        unconstrained_score(left)
            .partial_cmp(&unconstrained_score(right))
            .unwrap_or(std::cmp::Ordering::Equal)
    })?;
    if unconstrained.report.agent == selected.report.agent {
        return None;
    }
    if unconstrained.report.quota.status != "held" {
        return None;
    }
    Some(format!(
        "; {} held → {}",
        unconstrained.report.agent, selected.report.agent
    ))
}

fn unconstrained_score(item: &RankedCandidate) -> f64 {
    item.report.score
        - item.report.breakdown.rate_limit_penalty
        - item.report.breakdown.headroom_penalty
}

fn availability_notes(
    ranked: &[RankedCandidate],
    urgency: TaskUrgency,
    recommended: Option<&RecommendedAdvice>,
) -> Vec<String> {
    let targets: Vec<NoteTarget<'_>> = ranked
        .iter()
        .map(|item| {
            let custom = (item.order.kind == AgentKind::Custom).then_some(item.report.agent.as_str());
            NoteTarget {
                name: item.report.agent.as_str(),
                kind: item.order.kind,
                custom_name: custom,
            }
        })
        .collect();
    selection_quota::notes_for(&targets, urgency, recommended.map(|item| item.agent.as_str()))
}
