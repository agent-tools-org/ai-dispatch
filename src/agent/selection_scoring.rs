// Scoring internals for agent auto-selection.
// Exports: AGENT_CAPABILITIES, Candidate, CandidateContext, score_for, pick_best_candidate, etc.
// Deps: classifier, rate_limit, types.

use crate::agent::classifier::{self, Complexity, TaskCategory};
use crate::agent::custom::CustomAgentConfig;
use crate::cmd::config::AGENT_MODELS;
use crate::rate_limit;
use crate::team::TeamConfig;
use crate::types::AgentKind;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::process::Command;

pub(super) const AGENT_CAPABILITIES: &[(AgentKind, &[(TaskCategory, i32)])] = &[
    (AgentKind::Gemini, &[
        (TaskCategory::Research, 9), (TaskCategory::Documentation, 6),
        (TaskCategory::Debugging, 5), (TaskCategory::SimpleEdit, 2),
        (TaskCategory::ComplexImpl, 3), (TaskCategory::Frontend, 2),
        (TaskCategory::Testing, 3), (TaskCategory::Refactoring, 3),
    ]),
    (AgentKind::Qwen, &[
        (TaskCategory::Research, 9), (TaskCategory::Documentation, 6),
        (TaskCategory::Debugging, 5), (TaskCategory::SimpleEdit, 2),
        (TaskCategory::ComplexImpl, 3), (TaskCategory::Frontend, 2),
        (TaskCategory::Testing, 3), (TaskCategory::Refactoring, 3),
    ]),
    (AgentKind::Codex, &[
        (TaskCategory::ComplexImpl, 9), (TaskCategory::Refactoring, 8),
        (TaskCategory::Testing, 7), (TaskCategory::Debugging, 7),
        (TaskCategory::SimpleEdit, 4), (TaskCategory::Research, 1),
        (TaskCategory::Frontend, 4), (TaskCategory::Documentation, 3),
    ]),
    (AgentKind::Copilot, &[
        (TaskCategory::ComplexImpl, 8), (TaskCategory::Refactoring, 7),
        (TaskCategory::Testing, 7), (TaskCategory::Debugging, 7),
        (TaskCategory::SimpleEdit, 6), (TaskCategory::Research, 4),
        (TaskCategory::Frontend, 6), (TaskCategory::Documentation, 5),
    ]),
    (AgentKind::OpenCode, &[
        (TaskCategory::SimpleEdit, 8), (TaskCategory::Documentation, 5),
        (TaskCategory::Testing, 4), (TaskCategory::Debugging, 4),
        (TaskCategory::ComplexImpl, 3), (TaskCategory::Research, 1),
        (TaskCategory::Frontend, 2), (TaskCategory::Refactoring, 4),
    ]),
    (AgentKind::Kilo, &[
        (TaskCategory::SimpleEdit, 7), (TaskCategory::Documentation, 4),
        (TaskCategory::Testing, 3), (TaskCategory::Debugging, 3),
        (TaskCategory::ComplexImpl, 2), (TaskCategory::Research, 1),
        (TaskCategory::Frontend, 2), (TaskCategory::Refactoring, 3),
    ]),
    (AgentKind::Cursor, &[
        (TaskCategory::Frontend, 9), (TaskCategory::ComplexImpl, 7),
        (TaskCategory::Refactoring, 6), (TaskCategory::Testing, 5),
        (TaskCategory::Debugging, 5), (TaskCategory::SimpleEdit, 4),
        (TaskCategory::Research, 2), (TaskCategory::Documentation, 4),
    ]),
    (AgentKind::Codebuff, &[
        (TaskCategory::ComplexImpl, 8), (TaskCategory::Refactoring, 7),
        (TaskCategory::Frontend, 7), (TaskCategory::Testing, 6),
        (TaskCategory::Debugging, 6), (TaskCategory::SimpleEdit, 5),
        (TaskCategory::Research, 2), (TaskCategory::Documentation, 4),
    ]),
    (AgentKind::Droid, &[
        (TaskCategory::ComplexImpl, 9), (TaskCategory::Refactoring, 8),
        (TaskCategory::Testing, 7), (TaskCategory::Debugging, 7),
        (TaskCategory::SimpleEdit, 5), (TaskCategory::Research, 3),
        (TaskCategory::Frontend, 5), (TaskCategory::Documentation, 4),
    ]),
    (AgentKind::Oz, &[
        (TaskCategory::ComplexImpl, 8), (TaskCategory::Refactoring, 7),
        (TaskCategory::Testing, 6), (TaskCategory::Debugging, 6),
        (TaskCategory::SimpleEdit, 5), (TaskCategory::Research, 3),
        (TaskCategory::Frontend, 6), (TaskCategory::Documentation, 4),
    ]),
    (AgentKind::Claude, &[
        (TaskCategory::Research, 9), (TaskCategory::Documentation, 9),
        (TaskCategory::Debugging, 10), (TaskCategory::SimpleEdit, 5),
        (TaskCategory::ComplexImpl, 10), (TaskCategory::Frontend, 7),
        (TaskCategory::Testing, 10), (TaskCategory::Refactoring, 10),
    ]),
];

pub(super) fn base_score(agent: AgentKind, category: TaskCategory) -> i32 {
    AGENT_CAPABILITIES.iter()
        .find(|(k, _)| *k == agent)
        .and_then(|(_, scores)| scores.iter().find(|(c, _)| *c == category))
        .map(|(_, s)| *s).unwrap_or(1)
}

pub(super) fn priority(kind: AgentKind) -> i32 {
    match kind {
        AgentKind::Gemini | AgentKind::Qwen | AgentKind::Kilo => 0,
        AgentKind::OpenCode => 1,
        AgentKind::Copilot | AgentKind::Cursor | AgentKind::Codebuff => 2,
        AgentKind::Codex | AgentKind::Droid | AgentKind::Oz => 3,
        AgentKind::Claude => 3,
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

fn model_capability_score(agent: AgentKind, model: &str) -> Option<f64> {
    AGENT_MODELS.iter()
        .find(|m| m.agent == agent && m.model == model)
        .map(|m| m.capability)
}

pub(super) fn custom_category_score(config: &CustomAgentConfig, category: TaskCategory) -> i32 {
    let caps = &config.capabilities;
    match category {
        TaskCategory::Research => caps.research,
        TaskCategory::SimpleEdit => caps.simple_edit,
        TaskCategory::ComplexImpl => caps.complex_impl,
        TaskCategory::Frontend => caps.frontend,
        TaskCategory::Debugging => caps.debugging,
        TaskCategory::Testing => caps.testing,
        TaskCategory::Refactoring => caps.refactoring,
        TaskCategory::Documentation => caps.documentation,
    }
}

pub(super) fn category_strength_key(category: TaskCategory) -> &'static str {
    match category {
        TaskCategory::Research => "research",
        TaskCategory::SimpleEdit => "simple_edit",
        TaskCategory::ComplexImpl => "complex_impl",
        TaskCategory::Frontend => "frontend",
        TaskCategory::Debugging => "debugging",
        TaskCategory::Testing => "testing",
        TaskCategory::Refactoring => "refactoring",
        TaskCategory::Documentation => "documentation",
    }
}

pub(super) fn custom_strength_bonus(config: &CustomAgentConfig, category: TaskCategory) -> i32 {
    let key = category_strength_key(category);
    if config.strengths.iter().any(|s| s.eq_ignore_ascii_case(key)) {
        5
    } else {
        0
    }
}

pub(super) fn custom_command_installed(command: &str) -> bool {
    Command::new("which")
        .arg(command)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub(super) const BUILTIN_AGENTS: &[AgentKind] = AgentKind::ALL_BUILTIN;

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
}

pub(super) fn score_for(ctx: &CandidateContext<'_>, kind: AgentKind) -> f64 {
    let base = if let Some(tc) = ctx.team {
        team_override_score(tc, kind.as_str(), ctx.profile.category)
            .unwrap_or_else(|| base_score(kind, ctx.profile.category))
    } else {
        base_score(kind, ctx.profile.category)
    };
    let model = super::recommend_model(&kind, &ctx.profile.complexity, ctx.budget);
    let capability = model.and_then(|m| model_capability_score(kind, m));
    let mut s = model_quality_score(base, capability);
    if rate_limit::is_rate_limited(&kind) {
        s -= 10.0;
    }
    if let Some((rate, count)) = ctx.history_map.get(&kind)
        && *count >= 5
    {
        let bonus = ((*rate - 0.75) * 16.0).round().clamp(-5.0, 4.0);
        s += bonus;
    }
    if matches!(ctx.profile.complexity, Complexity::High)
        && matches!(
            kind,
            AgentKind::Codex | AgentKind::Copilot | AgentKind::Cursor | AgentKind::Droid | AgentKind::Oz | AgentKind::Claude
        )
    {
        s += 2.0;
    }
    // Boost preferred agents from team (soft preference, not hard filter)
    if let Some(tc) = ctx.team
        && tc
            .preferred_agents
            .iter()
            .any(|a| a.eq_ignore_ascii_case(kind.as_str()))
    {
        s += 3.0;
    }
    s
}

pub(super) fn candidate_for(kind: AgentKind, ctx: &CandidateContext<'_>) -> Candidate {
    let score = score_for(ctx, kind);
    let avg_cost = ctx.avg_cost_map.get(&kind).copied().unwrap_or(0.0);
    Candidate {
        kind,
        score,
        efficiency: cost_efficiency(score, avg_cost),
        is_default: ctx.team_default == Some(kind),
        priority: priority(kind),
    }
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

pub(super) fn pick_best_candidate(agents: &[AgentKind], ctx: &CandidateContext<'_>, budget: bool) -> Candidate {
    agents
        .iter()
        .map(|&kind| candidate_for(kind, ctx))
        .max_by(|a, b| compare_candidates(a, b, budget))
        .unwrap_or_else(|| candidate_for(AgentKind::Codex, ctx))
}

pub(super) fn team_override_score(team: &TeamConfig, agent_name: &str, category: TaskCategory) -> Option<i32> {
    let overrides = team.overrides.get(agent_name)?;
    match category {
        TaskCategory::Research => overrides.research,
        TaskCategory::SimpleEdit => overrides.simple_edit,
        TaskCategory::ComplexImpl => overrides.complex_impl,
        TaskCategory::Frontend => overrides.frontend,
        TaskCategory::Debugging => overrides.debugging,
        TaskCategory::Testing => overrides.testing,
        TaskCategory::Refactoring => overrides.refactoring,
        TaskCategory::Documentation => overrides.documentation,
    }
}
