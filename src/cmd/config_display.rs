// Display and history helpers for `aid config`.
// Exports: agent_profile(), format_capabilities(), models_for_agent(), budget_model()
// Deps: config_models, agent selection, rate_limit, task types

use std::cmp::Ordering;
use std::collections::HashMap;

use crate::agent::custom::CapabilityScores;
use crate::cost;
use crate::rate_limit;
use crate::types::{AgentKind, Task, TaskStatus};

use super::config_models::{AGENT_MODELS, AGENT_PROFILES, AgentModel};

pub(crate) struct AgentHistory {
    task_count: usize,
    success_rate: f64,
    avg_cost: f64,
}

pub(crate) struct ModelHistory {
    task_count: usize,
    success_rate: f64,
    avg_cost: f64,
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
    lines
}

fn render_rate_limit_line(kind: AgentKind) -> String {
    match rate_limit::get_rate_limit_info(&kind) {
        Some(info) if info.recovery_at.is_some() => {
            let fallback_hint = crate::agent::selection::coding_fallback_for(&kind)
                .map(|fallback| format!(" → use --fallback {}", fallback.as_str()))
                .unwrap_or_default();
            format!(
                "  Status:    rate-limited (try again at {}){}\n",
                info.recovery_at.as_deref().unwrap_or("unknown"),
                fallback_hint,
            )
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
            .filter(|task| matches!(task.status, TaskStatus::Done | TaskStatus::Merged))
            .count();
        let total_cost: f64 = agent_tasks.iter().filter_map(|task| task.cost_usd).sum();
        history.insert(
            agent,
            AgentHistory {
                task_count: agent_tasks.len(),
                success_rate: (done_count as f64 / agent_tasks.len() as f64) * 100.0,
                avg_cost: total_cost / agent_tasks.len() as f64,
            },
        );
    }
    history
}

pub(crate) fn compute_model_history(tasks: &[Task]) -> HashMap<(AgentKind, String), ModelHistory> {
    let mut accum: HashMap<(AgentKind, String), (usize, usize, f64)> = HashMap::new();
    for task in tasks {
        let model = task.model.clone().unwrap_or_else(|| "default".to_string());
        let entry = accum.entry((task.agent, model)).or_insert((0, 0, 0.0));
        entry.0 += 1;
        if matches!(task.status, TaskStatus::Done | TaskStatus::Merged) {
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

pub fn models_for_agent(agent: &AgentKind) -> Vec<&'static AgentModel> {
    AGENT_MODELS.iter().filter(|model| model.agent == *agent).collect()
}

pub fn budget_model(agent: &AgentKind) -> Option<&'static str> {
    let models = models_for_agent(agent);
    if models.is_empty() {
        return None;
    }
    let non_free: Vec<_> = models.iter().filter(|model| model.tier != "free").collect();
    if non_free.is_empty() {
        return models.first().map(|model| model.model);
    }
    non_free
        .iter()
        .min_by(|left, right| {
            let left_cost = left.input_per_m + left.output_per_m;
            let right_cost = right.input_per_m + right.output_per_m;
            left_cost.partial_cmp(&right_cost).unwrap_or(Ordering::Equal)
        })
        .map(|model| model.model)
}
