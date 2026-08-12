// Display and history helpers for `aid config`.
// Exports: agent_profile(), format_capabilities()
// Deps: model_catalog, agent selection, rate_limit, task types

use std::collections::{HashMap, HashSet};

use crate::agent::custom::CapabilityScores;
use crate::cost;
use crate::model_catalog::{AGENT_MODELS, AGENT_PROFILES};
use crate::rate_limit;
use crate::types::{AgentKind, Task, TaskOutcome};

pub(crate) struct AgentHistory {
    task_count: usize,
    success_rate: f64,
    avg_cost: f64,
}

pub(crate) struct ModelHistory {
    pub(crate) task_count: usize,
    pub(crate) success_rate: f64,
    pub(crate) avg_cost: f64,
}

pub(crate) fn format_capabilities(cap: &CapabilityScores) -> String {
    [
        ("research", cap.research),
        ("simple_edit", cap.simple_edit),
        ("complex_impl", cap.complex_impl),
        ("frontend", cap.frontend),
        ("debugging", cap.debugging),
        ("testing", cap.testing),
        ("refactoring", cap.refactoring),
        ("documentation", cap.documentation),
    ]
    .into_iter()
    .map(|(label, value)| format!("{label}={value}"))
    .collect::<Vec<_>>()
    .join(", ")
}

pub(crate) fn agent_profile(
    kind: AgentKind,
    installed: bool,
    history: Option<&AgentHistory>,
    model_history: &HashMap<(AgentKind, String), ModelHistory>,
) -> String {
    let profile = AGENT_PROFILES.iter().find(|(agent, _, _, _, _)| *agent == kind);
    let (strengths, cost, streaming) = match profile {
        Some((_, strengths, cost, _, streaming)) => (*strengths, *cost, *streaming),
        None => ("unknown", "unknown", false),
    };
    let mode = if streaming { "streaming" } else { "buffered" };
    let install_status = if installed { "installed" } else { "not installed" };
    let history_line = match history {
        Some(history) => format!(
            "  History:   {} tasks, {:.1}% success, avg {}/task\n",
            history.task_count,
            history.success_rate,
            cost::format_cost(Some(history.avg_cost))
        ),
        None => "  History:   no tasks yet\n".to_string(),
    };
    let models_line = render_models_line(kind, model_history);
    let rate_limit_line = render_rate_limit_line(kind);
    format!(
        "  Strengths: {}\n  Cost:      {}\n{}{}{}  Mode:      {} ({})\n",
        strengths, cost, history_line, rate_limit_line, models_line, mode, install_status
    )
}

pub(crate) fn recent_observed_models_line(
    kind: AgentKind,
    model_history: &HashMap<(AgentKind, String), ModelHistory>,
) -> Option<String> {
    let declared: HashSet<String> = AGENT_MODELS
        .iter()
        .filter(|model| model.agent == kind)
        .map(|model| model.model.to_lowercase())
        .collect();
    let mut extras: Vec<(&String, usize)> = model_history
        .iter()
        .filter_map(|((agent, model_key), hist)| {
            if *agent != kind {
                return None;
            }
            if model_key.eq_ignore_ascii_case("default") {
                return None;
            }
            if declared.contains(&model_key.to_lowercase()) {
                return None;
            }
            Some((model_key, hist.task_count))
        })
        .collect();
    if extras.is_empty() {
        return None;
    }
    extras.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    let parts: Vec<String> = extras
        .into_iter()
        .take(3)
        .map(|(name, count)| format!("{name} ({count})"))
        .collect();
    Some(format!("  Recent:    {}\n", parts.join(", ")))
}

fn render_models_line(
    kind: AgentKind,
    model_history: &HashMap<(AgentKind, String), ModelHistory>,
) -> String {
    let agent_models: Vec<_> = AGENT_MODELS.iter().filter(|model| model.agent == kind).collect();
    if agent_models.is_empty() {
        return "  Models:    none configured\n".to_string();
    }
    let mut lines = "  Models:\n".to_string();
    for model in &agent_models {
        let history_key = (kind, model.model.to_string());
        let history_suffix = match model_history.get(&history_key) {
            Some(history) => format!(
                "  [{} tasks, {:.0}% success, {}/task]",
                history.task_count,
                history.success_rate,
                cost::format_cost(Some(history.avg_cost))
            ),
            None => String::new(),
        };
        lines.push_str(&format!(
            "    {:<15} ({}, cap:{:.1}, ${:.2}/${:.2}/M)  {}{}\n",
            model.model,
            model.tier,
            model.capability,
            model.input_per_m,
            model.output_per_m,
            model.description,
            history_suffix
        ));
    }
    if let Some(recent) = recent_observed_models_line(kind, model_history) {
        lines.push_str(&recent);
    }
    lines
}

fn render_rate_limit_line(kind: AgentKind) -> String {
    if !rate_limit::is_rate_limited(&kind, None) {
        return String::new();
    }
    match rate_limit::get_rate_limit_info(&kind, None) {
        Some(info) => {
            let fallback_hint = crate::agent::selection::coding_fallback_for(&kind, None, None)
                .map(|fallback| format!(" → use --fallback {}", fallback.as_str()))
                .unwrap_or_default();
            let cause = match info.recovery_at {
                Some(recovery) => format!("try again at {recovery}"),
                None if info.needs_human => {
                    format!("needs manual clear: aid config clear-limit {}", kind.as_str())
                }
                None => "cooling down".to_string(),
            };
            format!("  Status:    rate-limited ({cause}){fallback_hint}\n")
        }
        _ => String::new(),
    }
}

pub(crate) fn compute_agent_history(tasks: &[Task]) -> HashMap<AgentKind, AgentHistory> {
    let mut history = HashMap::new();
    for &agent in AgentKind::ALL_BUILTIN {
        let agent_tasks: Vec<_> = tasks.iter().filter(|task| task.agent == agent).collect();
        if agent_tasks.is_empty() {
            continue;
        }
        let done_count = agent_tasks
            .iter()
            .filter(|task| task.outcome().is_success())
            .count();
        let total_cost: f64 = agent_tasks.iter().filter_map(|task| task.cost_usd).sum();
        let measured_count = agent_tasks
            .iter()
            .filter(|task| task.outcome() != TaskOutcome::Stopped)
            .count();
        history.insert(
            agent,
            AgentHistory {
                task_count: agent_tasks.len(),
                success_rate: if measured_count == 0 {
                    0.0
                } else {
                    (done_count as f64 / measured_count as f64) * 100.0
                },
                avg_cost: total_cost / agent_tasks.len() as f64,
            },
        );
    }
    history
}

pub(crate) fn compute_model_history(tasks: &[Task]) -> HashMap<(AgentKind, String), ModelHistory> {
    let mut accum: HashMap<(AgentKind, String), (usize, usize, f64)> = HashMap::new();
    for task in tasks {
        if task.outcome() == TaskOutcome::Stopped {
            continue;
        }
        let model = task.costing_model().unwrap_or("default").to_string();
        let entry = accum.entry((task.agent, model)).or_insert((0, 0, 0.0));
        entry.0 += 1;
        if task.outcome().is_success() {
            entry.1 += 1;
        }
        if let Some(cost) = task.cost_usd {
            entry.2 += cost;
        }
    }
    accum
        .into_iter()
        .map(|(key, (count, done, total_cost))| {
            (
                key,
                ModelHistory {
                    task_count: count,
                    success_rate: (done as f64 / count as f64) * 100.0,
                    avg_cost: total_cost / count as f64,
                },
            )
        })
        .collect()
}
