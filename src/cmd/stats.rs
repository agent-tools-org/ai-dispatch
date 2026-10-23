// Agent performance dashboard for the `aid stats` command.
// Exports: run(). Deps: crate::store::Store, crate::types, crate::cost::usage, crate::cost.

use crate::ui::format::format_tokens;
use anyhow::Result;
use chrono::{DateTime, Local, Timelike};
use std::collections::{BTreeMap, HashMap};

use super::stats_hint;
use crate::cost;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskOutcome};
use crate::cost::usage::UsageWindow;

#[derive(Debug, PartialEq)] struct StatsSnapshot { agent_rows: Vec<AgentRow>, failure_rows: Vec<FailureRow>, model_rows: Vec<ModelRow>, declared_rows: Vec<DeclaredRow>, activity_by_day: Vec<(String, usize)>, activity_by_hour: Vec<(u32, usize)>, top_sessions: Vec<TopSession>, total_cost: Option<f64>, total_tokens: i64, total_tasks: usize }
#[derive(Debug, PartialEq)] struct AgentRow { agent: String, tasks: usize, share_pct: usize, success_rate: f64, avg_duration_ms: Option<i64>, cost: String }
#[derive(Debug, PartialEq)] struct FailureRow { label: String, tasks: usize, agents: Vec<(String, usize)> }
#[derive(Debug, PartialEq)] struct ModelRow { model: String, tasks: usize, cost: String }
#[derive(Debug, PartialEq)] struct TopSession { task_id: String, agent: String, label: &'static str, value: String }
#[derive(Debug, PartialEq)] struct DeclaredRow { difficulty: String, tasks: usize, avg_duration_ms: Option<i64>, failures: usize }

pub fn run(store: &Store, window: String, agent: Option<String>, insights: bool) -> Result<()> {
    let window = UsageWindow::parse(&window)?;
    let stats = collect(store, window, agent.as_deref(), Local::now())?;
    print!("{}", render_output(&stats, window, insights, agent.is_some()));
    Ok(())
}

#[path = "stats/collect.rs"]
mod collection;

fn collect(
    store: &Store,
    window: UsageWindow,
    agent: Option<&str>,
    now: DateTime<Local>,
) -> Result<StatsSnapshot> {
    let tasks = store.read_report_tasks(None, agent, window.range(now))?;
    let mut run = collection::StatsRun::new();
    run.aggregate(store, &tasks)?;
    run.finish()
}

fn render_output(stats: &StatsSnapshot, window: UsageWindow, insights: bool, filtered_agent: bool) -> String {
    if stats.agent_rows.is_empty() {
        return format!("No tasks matched the selected filters for {}.\n", window.description());
    }
    render(stats, window, insights, filtered_agent)
}

fn render(stats: &StatsSnapshot, window: UsageWindow, insights: bool, filtered_agent: bool) -> String {
    let mut out = format!("Agent Performance ({})\n", window.description());
    for row in &stats.agent_rows { out.push_str(&format!("  {:<10} {:>3} tasks  {:>3}% share  {:>3.0}% success  avg {:<7}  {}\n", row.agent, row.tasks, row.share_pct, row.success_rate, format_duration(row.avg_duration_ms), row.cost)); }
    if stats.agent_rows.is_empty() { out.push_str("  (none)\n"); }
    out.push_str("\nTop Failure Causes\n");
    for (index, row) in stats.failure_rows.iter().enumerate() {
        let breakdown = row.agents.iter().map(|(agent, count)| format!("{agent}: {count}")).collect::<Vec<_>>().join(", ");
        out.push_str(&format!("  {}. {:<28} {:>3} tasks  ({})\n", index + 1, row.label, row.tasks, breakdown));
    }
    if stats.failure_rows.is_empty() { out.push_str("  (none)\n"); }
    out.push_str("\nModel Usage\n");
    for row in &stats.model_rows { out.push_str(&format!("  {:<18} {:>3} tasks  {}\n", row.model, row.tasks, row.cost)); }
    if stats.model_rows.is_empty() { out.push_str("  (none)\n"); }
    out.push_str("\nDeclared vs Outcome\n");
    for row in &stats.declared_rows { out.push_str(&format!("  {:<10} {:>3} tasks  avg {:<7}  {:>3} failed/verify-failed\n", row.difficulty, row.tasks, format_duration(row.avg_duration_ms), row.failures)); }
    if stats.declared_rows.is_empty() { out.push_str("  (no declared profiles)\n"); }
    out.push_str(&format!("\nOverview\n  Total: {} tasks  {} tokens  {}\n", stats.total_tasks, format_tokens(stats.total_tokens), cost::format_cost(stats.total_cost)));
    if let Some(hint) = stats.agent_rows.first().and_then(|row| stats_hint::diversification_hint(&row.agent, row.share_pct, stats.total_tasks, filtered_agent)) {
        out.push_str(&format!("  {hint}\n"));
    }
    if insights {
        push_bars(&mut out, "Activity by Day", &stats.activity_by_day);
        push_bars(&mut out, "Activity by Hour", &stats.activity_by_hour.iter().map(|(hour, count)| (format!("{hour:02}"), *count)).collect::<Vec<_>>());
    }
    out.push_str("\nTop Sessions\n");
    for row in &stats.top_sessions { out.push_str(&format!("  {:<13} {:<7} {:<7} {}\n", format!("{}:", row.label), row.task_id, row.agent, row.value)); }
    if stats.top_sessions.is_empty() { out.push_str("  (none)\n"); }
    out
}

fn push_bars(out: &mut String, title: &str, rows: &[(String, usize)]) {
    out.push_str(&format!("\n{title}\n"));
    let max = rows.iter().map(|(_, count)| *count).max().unwrap_or(0);
    for (label, count) in rows {
        let width = (count * 30).checked_div(max).unwrap_or(0);
        out.push_str(&format!("  {:<3} {:<30} {}\n", label, "█".repeat(width), count));
    }
}

fn classify_failure(detail: Option<&str>, exit_code: Option<i32>) -> String {
    let text = detail.unwrap_or("unknown failure");
    let lower = text.to_ascii_lowercase();
    if lower.contains("verify failed") { "verify failed".to_string() } else if lower.contains("hung: no output") { "agent hung: no output".to_string() } else if lower.contains("usage limit") || lower.contains("quota") { "usage limit".to_string() } else if let Some(idx) = lower.find("exit code ") {
        let suffix = &text[idx..];
        suffix.split_whitespace().take(3).collect::<Vec<_>>().join(" ")
    } else { exit_code.map(|code| format!("exit code {code}")).unwrap_or_else(|| text.to_string()) }
}

fn format_duration(duration_ms: Option<i64>) -> String {
    let secs = duration_ms.unwrap_or(0) / 1_000;
    match (secs / 60, secs % 60) { (0, s) => format!("{s}s"), (m, 0) => format!("{m}m"), (m, s) => format!("{m}m {s}s") }
}

fn usage_share(tasks: usize, total_tasks: usize) -> usize {
    (tasks * 100).checked_div(total_tasks).unwrap_or(0)
}

fn add_known_cost(total: &mut Option<f64>, cost: Option<f64>) {
    if let Some(cost) = cost {
        *total.get_or_insert(0.0) += cost;
    }
}

fn task_cost(store: &Store, task: &Task) -> Result<Option<f64>> {
    if let Some(cost) = task.cost_usd {
        if !matches!(task.agent, AgentKind::Codex | AgentKind::Claude) {
            return Ok(Some(cost));
        }
    }
    if matches!(task.agent, AgentKind::Cursor | AgentKind::Copilot) {
        return Ok(Some(0.0));
    }
    cost::task_cost(store, task)
}

#[cfg(test)]
#[path = "stats_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "stats/read_tests.rs"]
mod read_tests;
