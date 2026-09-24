// Recommendation pick, quota clause, and availability notes for advise.
// Exports: recommendation(), availability_notes(), run_model().
// Deps: ranked advise candidates, selection_quota notes, default-model resolution.

use std::collections::HashMap;

use super::super::selection_quota::{self, NoteTarget};
use super::{RankedCandidate, RecommendedAdvice};
use crate::agent::classifier::TaskCategory;
use crate::types::{AgentKind, DeclaredTaskProfile, TaskBudget, TaskUrgency};

/// First eligible candidate; else the best installed one not excluded as a
/// weaker model on the caller's pool; else the top of the list.
pub(super) fn recommendation(
    ranked: &[RankedCandidate], costs: &HashMap<AgentKind, f64>,
    durations: &HashMap<AgentKind, i64>, kind: TaskCategory, declared: DeclaredTaskProfile,
) -> Option<RecommendedAdvice> {
    let selected = ranked.iter().find(|item| item.report.eligible)
        .or_else(|| ranked.iter().find(|item| item.report.installed && !item.pool_excluded))
        .or_else(|| ranked.first())?;
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
        default_source: selected.report.default_source.clone(),
        score: selected.report.score, est_cost_usd: costs.get(&selected.order.kind).copied(),
        est_duration_secs: durations.get(&selected.order.kind).copied(),
        reason,
    })
}

/// The model `aid run` launches and, for an agent default, its source. Free
/// and cheap pin the catalog budget model. Standard and premium leave the
/// agent default unpinned: a sticky or CLI-configured default runs; with
/// neither, the budget-tier catalog row stands in as the catalog default.
pub(super) fn run_model(
    kind: AgentKind, budget: TaskBudget, catalog_model: Option<&str>,
) -> (Option<String>, Option<String>) {
    let catalog = catalog_model.map(str::to_string);
    if budget.uses_budget_mode() {
        return (catalog, None);
    }
    match crate::model_catalog::resolve_default_model(kind.as_str(), kind, None) {
        (Some(model), Some(source)) if source != "catalog" => (Some(model), Some(source.to_string())),
        (Some(model), Some(source)) => (catalog.or(Some(model)), Some(source.to_string())),
        _ => (catalog, None),
    }
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

pub(super) fn availability_notes(
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
