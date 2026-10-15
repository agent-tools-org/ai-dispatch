// Usage data structures and budget analytics for the `aid usage` command.
// Exports collection helpers and agent analytics without formatting.

use anyhow::Result;
use chrono::{DateTime, Duration, Local, LocalResult, TimeZone};

use crate::config::{AidConfig, UsageBudget};
use crate::store::Store;
use crate::types::{Task, TaskFilter, TaskOutcome};
use crate::cost::usage_report::{collect_agent_rows, collect_budget_rows, select_top_tasks};

mod types;
pub use types::{
    AgentAnalytics, AgentPeriodStats, AgentTrend, AgentTrendStats, BudgetStatus,
    TaskSummary, UsageReport, UsageSnapshot, UsageWindow,
};
pub(crate) use types::{AgentUsageRow, BudgetUsageRow};

pub fn collect_usage(store: &Store, config: &AidConfig) -> Result<UsageSnapshot> {
    let tasks = store.list_tasks(TaskFilter::All)?;
    collect_usage_from_tasks(&tasks, config)
}

pub fn collect_usage_from_tasks(tasks: &[Task], config: &AidConfig) -> Result<UsageSnapshot> {
    collect_usage_snapshot(tasks, config, UsageWindow::All, Local::now())
}

pub fn collect_usage_snapshot(
    tasks: &[Task],
    config: &AidConfig,
    window: UsageWindow,
    now: DateTime<Local>,
) -> Result<UsageSnapshot> {
    let filtered_tasks = filter_tasks_by_window(tasks, window, now);
    Ok(UsageSnapshot {
        generated_at: now,
        agent_rows: collect_agent_rows(&filtered_tasks),
        budget_rows: collect_budget_rows(tasks, &config.usage.budgets),
    })
}

mod budgets;
pub(crate) use budgets::check_budget_status_for_project;

pub fn agent_analytics(
    tasks: &[Task],
    agent_filter: &str,
    window: UsageWindow,
    now: DateTime<Local>,
) -> AgentAnalytics {
    let agent_key = agent_filter.trim();
    let matching_tasks: Vec<&Task> = tasks
        .iter()
        .filter(|task| task.agent_display_name().eq_ignore_ascii_case(agent_key))
        .collect();
    let display_name = matching_tasks
        .first()
        .map(|task| task.agent_display_name().to_string())
        .unwrap_or_else(|| agent_key.to_lowercase());
    let current_range = window.range(now);
    let current_tasks = filter_tasks_in_range(&matching_tasks, current_range);
    let stats = summarize_agent_period(&current_tasks);
    let cost_per_success = (stats.success_count > 0)
        .then(|| stats.cost_usd / stats.success_count as f64);
    let trend = window.previous_range(now).map(|range| {
        let previous_tasks = filter_tasks_in_range(&matching_tasks, Some(range));
        let previous_stats = summarize_agent_period(&previous_tasks);
        AgentTrend {
            label: window.previous_label(),
            current: AgentTrendStats {
                tasks: stats.tasks,
                cost_usd: stats.cost_usd,
            },
            previous: AgentTrendStats {
                tasks: previous_stats.tasks,
                cost_usd: previous_stats.cost_usd,
            },
        }
    });
    let top_tasks = select_top_tasks(&current_tasks);
    AgentAnalytics {
        agent_name: display_name,
        window,
        stats,
        cost_per_success,
        trend,
        top_tasks,
    }
}

fn summarize_agent_period(tasks: &[&Task]) -> AgentPeriodStats {
    let success_count = tasks
        .iter()
        .filter(|task| task.outcome().is_success())
        .count();
    let fail_count = tasks
        .iter()
        .filter(|task| matches!(task.outcome(), TaskOutcome::Broken | TaskOutcome::Failed))
        .count();
    let retry_count = tasks.iter().filter(|task| task.parent_task_id.is_some()).count();
    let tokens = tasks.iter().map(|task| task.tokens.unwrap_or(0)).sum();
    let cost_usd = tasks.iter().filter_map(|task| task.cost_usd).sum();
    let unknown_cost_tasks = crate::cost::unknown_cost_tasks(tasks.iter().copied());
    let durations: Vec<i64> = tasks
        .iter()
        .filter(|task| task.status.is_terminal())
        .filter_map(|task| task.duration_ms)
        .collect();
    let avg_duration_secs = if durations.is_empty() {
        0.0
    } else {
        durations.iter().sum::<i64>() as f64 / durations.len() as f64 / 1000.0
    };
    AgentPeriodStats {
        tasks: tasks.len(),
        success_count,
        fail_count,
        retry_count,
        tokens,
        cost_usd,
        unknown_cost_tasks,
        avg_duration_secs,
    }
}

fn filter_tasks_in_range<'a>(
    tasks: &'a [&'a Task],
    range: Option<(DateTime<Local>, DateTime<Local>)>,
) -> Vec<&'a Task> {
    match range {
        Some((start, end)) => tasks
            .iter()
            .copied()
            .filter(|task| task.created_at >= start && task.created_at <= end)
            .collect(),
        None => tasks.to_vec(),
    }
}

fn filter_tasks_by_window(tasks: &[Task], window: UsageWindow, now: DateTime<Local>) -> Vec<Task> {
    match window.range(now) {
        None => tasks.to_vec(),
        Some((start, end)) => tasks
            .iter()
            .filter(|task| task.created_at >= start && task.created_at <= end)
            .cloned()
            .collect(),
    }
}

pub(crate) fn filter_budget_tasks<'a>(tasks: &'a [Task], budget: &UsageBudget) -> Vec<&'a Task> {
    let window_start = budget
        .window
        .as_deref()
        .and_then(parse_window)
        .map(|window| Local::now() - window);

    tasks
        .iter()
        .filter(|task| {
            let scope_matches = match budget.agent.as_deref() {
                Some(name) => task.agent_display_name() == name,
                None => task.project_id.as_deref() == Some(budget.name.as_str()),
            };
            let window_matches = window_start
                .map(|start| task.created_at >= start)
                .unwrap_or(true);
            scope_matches && window_matches
        })
        .collect()
}

pub(crate) fn parse_window(value: &str) -> Option<Duration> {
    let trimmed = value.trim();
    match trimmed.to_lowercase().as_str() {
        "daily" | "day" => return Some(Duration::days(1)),
        "weekly" | "week" => return Some(Duration::days(7)),
        "monthly" | "month" => return Some(Duration::days(30)),
        _ => {}
    }
    if let Some(hours) = trimmed.strip_suffix('h') {
        return hours.parse::<i64>().ok().map(Duration::hours);
    }
    if let Some(days) = trimmed.strip_suffix('d') {
        return days.parse::<i64>().ok().map(Duration::days);
    }
    if let Some(minutes) = trimmed.strip_suffix('m') {
        return minutes.parse::<i64>().ok().map(Duration::minutes);
    }
    None
}

fn start_of_day(now: DateTime<Local>) -> DateTime<Local> {
    let date = now.date_naive();
    let Some(naive) = date.and_hms_opt(0, 0, 0) else { return now; };
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(dt) => dt,
        LocalResult::Ambiguous(dt, _) => dt,
        LocalResult::None => Local.from_utc_datetime(&naive),
    }
}

#[cfg(test)]
#[path = "usage_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "usage_analytics_tests.rs"]
mod analytics_tests;

#[cfg(test)]
#[path = "usage_window_tests.rs"]
mod window_tests;

#[cfg(test)]
#[path = "usage_cost_tests.rs"]
mod cost_tests;
