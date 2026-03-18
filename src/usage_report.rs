// Render helpers for usage snapshots and agent analytics.
// Exports render_usage(), render_agent_analytics(), and formatting utilities.

use chrono::{DateTime, Local};
use std::cmp::Ordering;

use crate::config::UsageBudget;
use crate::cost;
use crate::paths;
use crate::types::{AgentKind, Task, TaskStatus};
use crate::usage::{filter_budget_tasks, AgentAnalytics, AgentUsageRow, BudgetUsageRow, TaskSummary, UsageSnapshot};

pub fn render_usage(snapshot: &UsageSnapshot) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Usage snapshot at {}\n",
        snapshot.generated_at.format("%Y-%m-%d %H:%M:%S %:z"),
    ));

    if !snapshot.agent_rows.is_empty() {
        out.push_str("\nTracked Task History\n");
        out.push_str(&format!(
            "{:<12} {:<8} {:<10} {:<10} {:<6} {:<8} {:<7} {}\n",
            "Agent", "Tasks", "Tokens", "Cost", "Success%", "Avg Time", "Retries", "Last Task"
        ));
        out.push_str(&"-".repeat(80));
        out.push('\n');
        for row in &snapshot.agent_rows {
            out.push_str(&format!(
                "{:<12} {:<8} {:<10} {:<10} {:<6} {:<8} {:<7} {}\n",
                row.name,
                row.tasks,
                format_tokens(row.tokens),
                cost::format_cost(Some(row.cost_usd)),
                format!("{:.1}%", row.success_rate),
                format_duration_secs(row.avg_duration_secs),
                row.retry_count,
                row.last_task_at
                    .map(format_last_seen)
                    .unwrap_or_else(|| "-".to_string()),
            ));
        }
    }

    if snapshot.budget_rows.is_empty() {
        out.push_str(&format!(
            "\nNo budgets configured. Add `[[usage.budget]]` entries to {}.\n",
            paths::config_path().display(),
        ));
        return out;
    }

    out.push_str("\nConfigured Budgets\n");
    out.push_str(&format!(
        "{:<16} {:<8} {:<8} {:<12} {:<14} {:<14} {:<14} {}\n",
        "Name", "Plan", "Window", "Tasks", "Tokens", "Cost", "Requests", "Resets"
    ));
    out.push_str(&"-".repeat(118));
    out.push('\n');
    for row in &snapshot.budget_rows {
        out.push_str(&format!(
            "{:<16} {:<8} {:<8} {:<12} {:<14} {:<14} {:<14} {}\n",
            row.name,
            row.plan.as_deref().unwrap_or("-"),
            row.window.as_deref().unwrap_or("-"),
            format_ratio_u32(row.tasks, row.task_limit),
            format_ratio_i64(row.tokens, row.token_limit, format_tokens),
            format_ratio_f64(row.cost_usd, row.cost_limit_usd),
            format_ratio_u32(row.requests, row.request_limit),
            row.resets_at.as_deref().unwrap_or("-"),
        ));
        if let Some(notes) = row.notes.as_deref() {
            out.push_str(&format!("  note: {notes}\n"));
        }
    }
    out
}

pub fn render_agent_analytics(analytics: &AgentAnalytics) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Agent '{agent}' usage ({period})\n",
        agent = analytics.agent_name,
        period = analytics.window.description()
    ));
    out.push_str(&format!(
        "Tasks: {} (success {}, fail {}, retries {})\n",
        analytics.stats.tasks,
        analytics.stats.success_count,
        analytics.stats.fail_count,
        analytics.stats.retry_count
    ));
    out.push_str(&format!(
        "Tokens: {} | Cost: {} | Avg duration: {}\n",
        format_tokens(analytics.stats.tokens),
        cost::format_cost(Some(analytics.stats.cost_usd)),
        format_duration_secs(analytics.stats.avg_duration_secs)
    ));
    let cost_per_success = analytics
        .cost_per_success
        .map(|cost| cost::format_cost(Some(cost)))
        .unwrap_or_else(|| "-".to_string());
    out.push_str(&format!("Cost per success: {}\n", cost_per_success));
    if let Some(trend) = &analytics.trend {
        let task_delta = trend.current.tasks as isize - trend.previous.tasks as isize;
        let cost_delta = trend.current.cost_usd - trend.previous.cost_usd;
        let cost_delta_label = format!(
            "{}{}",
            if cost_delta >= 0.0 { '+' } else { '-' },
            cost::format_cost(Some(cost_delta.abs()))
        );
        out.push_str(&format!(
            "Trend vs {}: tasks {:+}, cost {}\n",
            trend.label, task_delta, cost_delta_label
        ));
    }
    out.push_str("Top 5 most expensive tasks:\n");
    if analytics.top_tasks.is_empty() {
        out.push_str("  (no tasks recorded in this period)\n");
    } else {
        for task in &analytics.top_tasks {
            let duration = task
                .duration_secs
                .map(format_duration_secs)
                .unwrap_or_else(|| "-".to_string());
            out.push_str(&format!(
                "  {} | {} | {} | {}\n",
                task.id,
                cost::format_cost(Some(task.cost_usd)),
                duration,
                task.prompt_snippet
            ));
        }
    }
    out
}

pub(crate) fn collect_agent_rows(tasks: &[Task]) -> Vec<AgentUsageRow> {
    let mut rows = Vec::new();
    for agent in [
        AgentKind::Codex,
        AgentKind::Gemini,
        AgentKind::OpenCode,
        AgentKind::Kilo,
        AgentKind::Cursor,
        AgentKind::Droid,
    ] {
        let agent_tasks: Vec<&Task> = tasks.iter().filter(|task| task.agent == agent).collect();
        if agent_tasks.is_empty() {
            continue;
        }

        let done_count = agent_tasks
            .iter()
            .filter(|task| matches!(task.status, TaskStatus::Done | TaskStatus::Merged))
            .count();
        let retry_count = agent_tasks
            .iter()
            .filter(|task| task.parent_task_id.is_some())
            .count();
        let completed_durations: Vec<i64> = agent_tasks
            .iter()
            .filter(|task| task.status.is_terminal())
            .filter_map(|task| task.duration_ms)
            .collect();
        let success_rate = (done_count as f64 * 100.0) / agent_tasks.len() as f64;
        let avg_duration_secs = if completed_durations.is_empty() {
            0.0
        } else {
            completed_durations.iter().sum::<i64>() as f64
                / completed_durations.len() as f64
                / 1000.0
        };
        let tokens = agent_tasks.iter().filter_map(|task| task.tokens).sum();
        let cost_usd = agent_tasks.iter().filter_map(|task| task.cost_usd).sum();
        let last_task_at = agent_tasks.iter().map(|task| task.created_at).max();
        rows.push(AgentUsageRow {
            name: agent.as_str(),
            tasks: agent_tasks.len(),
            tokens,
            cost_usd,
            success_rate,
            avg_duration_secs,
            retry_count,
            last_task_at,
        });
    }
    rows
}

pub(crate) fn collect_budget_rows(tasks: &[Task], budgets: &[UsageBudget]) -> Vec<BudgetUsageRow> {
    budgets
        .iter()
        .map(|budget| {
            let budget_tasks = filter_budget_tasks(tasks, budget);
            let tasks_used = budget_tasks.len() as u32 + budget.external_tasks;
            let tokens_used = budget_tasks
                .iter()
                .filter_map(|task| task.tokens)
                .sum::<i64>()
                + budget.external_tokens;
            let cost_used = budget_tasks
                .iter()
                .filter_map(|task| task.cost_usd)
                .sum::<f64>()
                + budget.external_cost_usd;
            BudgetUsageRow {
                name: budget.name.clone(),
                plan: budget.plan.clone(),
                window: budget.window.clone(),
                tasks: tasks_used,
                task_limit: budget.task_limit,
                tokens: tokens_used,
                token_limit: budget.token_limit,
                cost_usd: cost_used,
                cost_limit_usd: budget.cost_limit_usd,
                requests: budget.external_requests,
                request_limit: budget.request_limit,
                resets_at: budget.resets_at.clone(),
                notes: budget.notes.clone(),
            }
        })
        .collect()
}

pub(crate) fn select_top_tasks(tasks: &[&Task]) -> Vec<TaskSummary> {
    let mut top: Vec<&Task> = tasks.to_vec();
    top.sort_by(|a, b| {
        let a_cost = a.cost_usd.unwrap_or(0.0);
        let b_cost = b.cost_usd.unwrap_or(0.0);
        b_cost
            .partial_cmp(&a_cost)
            .unwrap_or(Ordering::Equal)
    });
    top.truncate(5);
    top.into_iter()
        .map(|task| TaskSummary {
            id: task.id.to_string(),
            prompt_snippet: prompt_snippet(task),
            cost_usd: task.cost_usd.unwrap_or(0.0),
            duration_secs: task.duration_ms.map(|ms| ms as f64 / 1000.0),
        })
        .collect()
}

fn prompt_snippet(task: &Task) -> String {
    let source = task.resolved_prompt.as_deref().unwrap_or(&task.prompt);
    let trimmed = source.lines().next().unwrap_or("").trim();
    if trimmed.chars().count() <= 80 {
        trimmed.to_string()
    } else {
        trimmed.chars().take(77).collect::<String>() + "..."
    }
}

fn format_last_seen(timestamp: DateTime<Local>) -> String {
    let elapsed = Local::now() - timestamp;
    let secs = elapsed.num_seconds();
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else {
        format!("{}h ago", secs / 3600)
    }
}

fn format_duration_secs(seconds: f64) -> String {
    let secs = seconds.round() as i64;
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}

fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

fn format_ratio_u32(current: u32, limit: Option<u32>) -> String {
    limit
        .map(|limit| format!("{current}/{limit}"))
        .unwrap_or_else(|| current.to_string())
}

fn format_ratio_i64(current: i64, limit: Option<i64>, formatter: fn(i64) -> String) -> String {
    limit
        .map(|limit| format!("{}/{}", formatter(current), formatter(limit)))
        .unwrap_or_else(|| formatter(current))
}

fn format_ratio_f64(current: f64, limit: Option<f64>) -> String {
    limit
        .map(|limit| {
            format!(
                "{}/{}",
                cost::format_cost(Some(current)),
                cost::format_cost(Some(limit))
            )
        })
        .unwrap_or_else(|| cost::format_cost(Some(current)))
}
