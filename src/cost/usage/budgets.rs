// Project and per-agent usage-budget admission policy.
// Uses persisted usage summaries and configured limits.
use super::{AidConfig, BudgetStatus, Store, UsageBudget, parse_window};
use anyhow::Result;
use chrono::{DateTime, Local};

/// Check the dispatch target identity supplied by the caller; None has no project cap.
pub(crate) fn check_budget_status_for_project(
    store: &Store,
    config: &AidConfig,
    project_name: Option<&str>,
) -> Result<BudgetStatus> {
    let now = Local::now();
    let mut over_limit = false;
    let mut near_limit = false;
    let mut messages: Vec<String> = Vec::new();
    let mut warned_unrecognized_window = false;

    for budget in &config.usage.budgets {
        if budget.agent.is_none() && Some(budget.name.as_str()) != project_name {
            continue;
        }
        let since = budget
            .window
            .as_deref()
            .and_then(|window| {
                let trimmed = window.trim();
                if trimmed.is_empty() {
                    return None;
                }
                let parsed = parse_window(trimmed);
                if parsed.is_none() && !warned_unrecognized_window {
                    crate::aid_warn!(
                        "[aid] Warning: budget '{}' has unrecognized window '{}'; treating budget as all-time (lifetime)",
                        budget.name,
                        trimmed
                    );
                    warned_unrecognized_window = true;
                }
                parsed
            })
            .map(|window| now - window);
        let (task_count, total_tokens, total_cost) = budget_usage_summary(store, budget, since)?;
        let tasks = task_count + budget.external_tasks;
        let tokens = total_tokens + budget.external_tokens;
        let cost_usd = total_cost + budget.external_cost_usd;

        if let Some(limit) = budget.task_limit {
            if tasks >= limit {
                over_limit = true;
                messages.push(format!(
                    "Budget '{}': task limit reached ({}/{})",
                    budget.name, tasks, limit
                ));
            } else if tasks as f64 / limit as f64 > 0.8 {
                near_limit = true;
                messages.push(format!(
                    "Budget '{}': task usage at {}%",
                    budget.name,
                    (tasks as f64 / limit as f64 * 100.0) as u32
                ));
            }
        }
        if let Some(limit) = budget.cost_limit_usd {
            if cost_usd >= limit {
                over_limit = true;
                messages.push(format!(
                    "Budget '{}': cost limit reached (${:.2}/${:.2})",
                    budget.name, cost_usd, limit
                ));
            } else if cost_usd / limit > 0.8 {
                near_limit = true;
                messages.push(format!(
                    "Budget '{}': cost usage at {}%",
                    budget.name,
                    (cost_usd / limit * 100.0) as u32
                ));
            }
        }
        if let Some(limit) = budget.token_limit {
            if tokens >= limit {
                over_limit = true;
                messages.push(format!(
                    "Budget '{}': token limit reached ({}/{})",
                    budget.name, tokens, limit
                ));
            } else if tokens as f64 / limit as f64 > 0.8 {
                near_limit = true;
                messages.push(format!(
                    "Budget '{}': token usage at {}%",
                    budget.name,
                    (tokens as f64 / limit as f64 * 100.0) as u32
                ));
            }
        }
    }

    Ok(BudgetStatus {
        over_limit,
        near_limit,
        message: if messages.is_empty() {
            None
        } else {
            Some(messages.join("\n"))
        },
    })
}

fn budget_usage_summary(
    store: &Store,
    budget: &UsageBudget,
    since: Option<DateTime<Local>>,
) -> Result<(u32, i64, f64)> {
    match budget.agent.as_deref() {
        Some(agent) => store.budget_usage_summary(agent, since),
        None => store.budget_usage_summary_for_project(&budget.name, since),
    }
}
