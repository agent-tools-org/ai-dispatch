// Auto-selection heuristics for `aid run auto`.
// Scores prompt signals via capability matrix, respects installed CLIs, returns concise reason.
// Exports select_agent() helpers; deps: super::detect_agents, super::RunOpts.

#[path = "selection_scoring.rs"]
mod selection_scoring;
use selection_scoring::{
    BUILTIN_AGENTS, Candidate, CandidateContext, candidate_for, compare_candidates, cost_efficiency,
    custom_category_score, custom_command_installed, custom_strength_bonus, pick_best_candidate,
    priority,
};
use super::classifier::{self, Complexity, TaskCategory};
use super::{detect_agents, RunOpts};
use crate::rate_limit;
use crate::store::Store;
use crate::types::{AgentKind, TaskStatus};
use crate::agent::registry::load_custom_agents;
use crate::team::TeamConfig;
use std::collections::HashMap;

pub(crate) const AGENT_CAPABILITIES: &[(AgentKind, &[(TaskCategory, i32)])] =
    selection_scoring::AGENT_CAPABILITIES;

pub(crate) fn select_agent_with_reason(
    prompt: &str, opts: &RunOpts, store: &Store,
    team: Option<&TeamConfig>,
) -> (String, String) {
    let available = detect_agents();
    select_agent_from(prompt, opts, &available, store, team)
}

fn select_agent_from(
    prompt: &str, opts: &RunOpts, available: &[AgentKind],
    store: &Store,
    team: Option<&TeamConfig>,
) -> (String, String) {
    let normalized = prompt.trim().to_lowercase();
    let prompt_len = prompt.chars().count();
    let file_count = classifier::count_file_mentions(&normalized);
    let profile = classifier::classify(prompt, file_count, prompt_len);
    let auto_budget = classifier::contains_any(&normalized, classifier::LOW_VALUE_TERMS);
    let budget = opts.budget || auto_budget;
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
        budget,
    };
    let primary_candidate = pick_best_candidate(BUILTIN_AGENTS, &ctx, budget);
    let available_candidate = if available.is_empty() {
        pick_best_candidate(BUILTIN_AGENTS, &ctx, budget)
    } else {
        pick_best_candidate(available, &ctx, budget)
    };
    let mut selected_name = available_candidate.kind.as_str().to_string();
    let mut selected_score = available_candidate.score;
    let mut selected_builtin = Some(available_candidate.kind);
    if let Some((custom_name, custom_score)) = load_custom_agents()
        .into_values()
        .filter(|config| AgentKind::parse_str(&config.id).is_none())
        .filter(|config| custom_command_installed(&config.command))
        .map(|config| {
            let mut score = custom_category_score(&config, profile.category);
            score += custom_strength_bonus(&config, profile.category);
            // Boost preferred custom agents from team
            if let Some(tc) = &team
                && tc.preferred_agents.iter().any(|a| a.eq_ignore_ascii_case(&config.id))
            {
                score += 3;
            }
            (config.id, score)
        })
        .max_by_key(|(_, score)| *score)
    {
        let custom_score_value = custom_score as f64;
        let custom_priority = priority(AgentKind::Custom);
        let available_priority = priority(available_candidate.kind);
        if (custom_score_value > available_candidate.score)
            || (custom_score_value == available_candidate.score && custom_priority > available_priority)
        {
            selected_name = custom_name;
            selected_score = custom_score_value;
            selected_builtin = None;
        }
    }
    let selected_model = selected_builtin
        .and_then(|kind| recommend_model(&kind, &profile.complexity, budget));
    let selected_avg_cost = selected_builtin
        .and_then(|kind| avg_cost_map.get(&kind).copied())
        .unwrap_or(0.0);
    let selected_efficiency = cost_efficiency(selected_score, selected_avg_cost);
    let selected_model_label = if let Some(model) = selected_model {
        format!("{}/{}", selected_name, model)
    } else {
        selected_name.clone()
    };
    let selected_label = if budget && selected_builtin.is_some() {
        format!(
            "{} (score: {:.1}, avg: ${:.2}, efficiency: {:.1})",
            selected_model_label, selected_score, selected_avg_cost, selected_efficiency
        )
    } else {
        format!("{} (score: {:.1})", selected_model_label, selected_score)
    };
    let mut reason = format!(
        "{} task ({}) \u{2192} {}",
        profile.category.label(), profile.complexity.label(),
        selected_label,
    );
    if let Some(sel_kind) = selected_builtin
        && sel_kind != primary_candidate.kind
    {
        reason.push_str(&format!("; {} unavailable", primary_candidate.kind.as_str()));
    }
    if auto_budget {
        reason.push_str("; auto-budget: low-value task");
    } else if opts.budget {
        reason.push_str("; budget mode");
    }
    if rate_limit::is_rate_limited(&AgentKind::Codex) && selected_name != AgentKind::Codex.as_str() {
        reason.push_str("; codex rate-limited");
    }
    if let Some(sel_kind) = selected_builtin
        && let Some((rate, count)) = history_map.get(&sel_kind)
        && *count >= 5
    {
        let percent = (*rate * 100.0).round() as i32;
        let success_label = format!("{:.0}%", rate * 100.0);
        reason.push_str(&format!("; history: {}% success (success: {})", percent, success_label));
    }
    if let Ok(similar_tasks) = store.find_similar_tasks(prompt, 5) {
        let mut stats: HashMap<AgentKind, (usize, usize)> = HashMap::new();
        for (_, agent, status) in similar_tasks {
            let entry = stats.entry(agent).or_insert((0, 0));
            entry.1 += 1;
            if matches!(status, TaskStatus::Done | TaskStatus::Merged) {
                entry.0 += 1;
            }
        }
        if let Some((&agent, &(successes, total))) = stats.iter().max_by(|a, b| {
            a.1 .0.cmp(&b.1 .0).then(a.1 .1.cmp(&b.1 .1))
        })
            && successes >= 3
        {
            reason.push_str(&format!(
                "; similar tasks: {} {}/{} success",
                agent.as_str(),
                successes,
                total,
            ));
        }
    }
    (selected_name, reason)
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
    };
    let mut candidates: Vec<Candidate> = BUILTIN_AGENTS
        .iter()
        .map(|&kind| candidate_for(kind, &ctx))
        .collect();
    candidates.sort_by(|a, b| compare_candidates(a, b, true).reverse());
    candidates.into_iter().map(|c| c.kind).collect()
}

pub(crate) fn recommend_model(
    agent: &AgentKind, complexity: &Complexity, budget: bool,
) -> Option<&'static str> {
    use crate::cmd::config::{budget_model, models_for_agent};
    if budget { return budget_model(agent); }
    let models = models_for_agent(agent);
    if models.is_empty() { return None; }
    let tier = match complexity {
        Complexity::Low => "cheap", Complexity::Medium => "standard", Complexity::High => "premium",
    };
    models.iter().find(|m| m.tier == tier).or_else(|| models.first()).map(|m| m.model)
}

const CODING_FALLBACK_CHAIN: &[AgentKind] = &[
    AgentKind::Gemini, AgentKind::Codex, AgentKind::Cursor, AgentKind::Droid, AgentKind::OpenCode, AgentKind::Kilo,
];
pub(crate) fn coding_fallback_for(agent: &AgentKind) -> Option<AgentKind> {
    let available = detect_agents();
    let start = CODING_FALLBACK_CHAIN.iter().position(|k| k == agent)?;
    CODING_FALLBACK_CHAIN[start + 1..]
        .iter()
        .find(|k| available.contains(k) && !rate_limit::is_rate_limited(k))
        .copied()
}

#[cfg(test)]
mod tests;
