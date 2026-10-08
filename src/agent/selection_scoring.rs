// Scoring internals for agent auto-selection.
// Exports: Candidate, CandidateContext, ScoreBreakdown, comparison helpers.
// Deps: classifier, capability matrix, model catalog, rate limits, task profiles.

use crate::agent::classifier::{self, TaskCategory};
use crate::route_availability::{availability_for_model, RouteStatus};
use crate::team::TeamConfig;
use crate::types::AgentKind;
use std::cmp::Ordering;
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
pub(super) use super::selection_capabilities::{
    base_score, team_override_score,
};

pub(super) fn priority(kind: AgentKind) -> i32 {
    match kind {
        AgentKind::Gemini
        | AgentKind::Antigravity
        | AgentKind::Qwen
        | AgentKind::Kilo
        | AgentKind::MiMoCode => 0,
        AgentKind::OpenCode => 1,
        AgentKind::Copilot | AgentKind::Cursor => 2,
        AgentKind::Codex | AgentKind::CommandCode | AgentKind::Droid | AgentKind::Oz => 3,
        AgentKind::Claude | AgentKind::Grok => 3,
        AgentKind::Custom => 1,
    }
}

pub(super) fn cost_efficiency(quality_score: f64, avg_cost: f64) -> f64 {
    let normalized_cost = avg_cost.max(0.0);
    quality_score / (1.0 + normalized_cost)
}

pub(super) fn model_quality_score(base_score: i32, capability: Option<f64>) -> f64 {
    let base = base_score.max(0) as f64;
    if let Some(cap) = capability {
        (base + cap) * 0.5
    } else {
        base
    }
}

pub(super) fn model_capability_score(agent: AgentKind, model: &str, category: TaskCategory) -> Option<f64> {
    crate::scores::capability_score(agent, model, category)
}

/// True when the model has a non-zero price. Used to bias budget mode toward
/// free models so a marginally stronger paid model doesn't win trivial tasks.
fn model_is_paid(agent: AgentKind, model: &str) -> bool {
    crate::cost::resolve_pricing(Some(model), agent)
        .is_some_and(|price| price.input_per_m > 0.0 || price.output_per_m > 0.0)
}


#[derive(Clone)]
pub(super) struct Candidate {
    pub(super) kind: AgentKind,
    pub(super) score: f64,
    pub(super) efficiency: f64,
    pub(super) is_default: bool,
    pub(super) priority: i32,
}

pub(super) struct CandidateContext<'a> {
    pub(super) profile: &'a classifier::TaskProfile,
    pub(super) team: Option<&'a TeamConfig>,
    pub(super) history_map: &'a HashMap<AgentKind, (f64, usize)>,
    pub(super) avg_cost_map: &'a HashMap<AgentKind, f64>,
    pub(super) team_default: Option<AgentKind>,
    pub(super) budget: bool,
    pub(super) penalize_rate_limit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) struct ScoreBreakdown {
    pub base: f64,
    pub model_capability: f64,
    pub budget_penalty: f64,
    pub rate_limit_penalty: f64,
    pub history_bonus: f64,
    pub team_bonus: f64,
    #[serde(default)]
    pub headroom_penalty: f64,
    pub total: f64,
}

/// Score for `kind` running `model`. An unknown or unrated model scores the
/// agent-level base with no model capability term.
pub(super) fn score_breakdown(
    ctx: &CandidateContext<'_>,
    kind: AgentKind,
    model: Option<&str>,
) -> ScoreBreakdown {
    let (base, initial) = initial_score(ctx, kind, model);
    let mut s = initial;
    let mut budget_penalty = 0.0;
    // Budget mode favors free models: a paid agent must be clearly stronger to
    // win, so trivial tasks route to free agents (kilo/qwen/free opencode).
    if ctx.budget && model.is_some_and(|m| model_is_paid(kind, m)) {
        s -= 3.0;
        budget_penalty = -3.0;
    }
    let mut rate_limit_penalty = 0.0;
    if ctx.penalize_rate_limit && availability_for_model(&kind, None, model).status == RouteStatus::Held {
        s -= 10.0;
        rate_limit_penalty = -10.0;
    }
    let mut history_bonus = 0.0;
    if let Some(bonus) = history_score_bonus(ctx, kind) {
        s += bonus;
        history_bonus = bonus;
    }
    // Boost preferred agents from team (soft preference, not hard filter)
    let mut team_bonus = 0.0;
    if has_team_bonus(ctx, kind) {
        s += 3.0;
        team_bonus = 3.0;
    }
    let headroom_penalty = super::selection_quota::headroom_penalty(kind, model);
    if headroom_penalty != 0.0 { s += headroom_penalty; } // 0.0 would change bits
    ScoreBreakdown {
        base: base as f64,
        model_capability: initial - base as f64,
        budget_penalty,
        rate_limit_penalty,
        history_bonus,
        team_bonus,
        headroom_penalty,
        total: s,
    }
}

fn initial_score(ctx: &CandidateContext<'_>, kind: AgentKind, model: Option<&str>) -> (i32, f64) {
    let base = ctx.team
        .and_then(|team| team_override_score(team, kind.as_str(), ctx.profile.category))
        .unwrap_or_else(|| base_score(kind, ctx.profile.category));
    let capability = model.and_then(|value| model_capability_score(kind, value, ctx.profile.category));
    (base, model_quality_score(base, capability))
}

fn history_score_bonus(ctx: &CandidateContext<'_>, kind: AgentKind) -> Option<f64> {
    let (rate, count) = ctx.history_map.get(&kind)?;
    (*count >= 5).then(|| ((*rate - 0.75) * 16.0).round().clamp(-5.0, 4.0))
}

fn has_team_bonus(ctx: &CandidateContext<'_>, kind: AgentKind) -> bool {
    ctx.team.is_some_and(|team| team.preferred_agents.iter()
        .any(|agent| agent.eq_ignore_ascii_case(kind.as_str())))
}

pub(super) fn compare_candidates(a: &Candidate, b: &Candidate, budget: bool) -> Ordering {
    let primary = if budget {
        a.efficiency.partial_cmp(&b.efficiency).unwrap_or(Ordering::Equal)
    } else {
        a.score.partial_cmp(&b.score).unwrap_or(Ordering::Equal)
    };
    let mut ord = primary;
    if ord == Ordering::Equal {
        ord = if budget {
            a.score.partial_cmp(&b.score).unwrap_or(Ordering::Equal)
        } else {
            a.efficiency
                .partial_cmp(&b.efficiency)
                .unwrap_or(Ordering::Equal)
        };
    }
    if ord == Ordering::Equal {
        ord = a.is_default.cmp(&b.is_default);
    }
    if ord == Ordering::Equal {
        ord = a.priority.cmp(&b.priority);
    }
    ord
}
