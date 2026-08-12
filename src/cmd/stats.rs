// Agent performance dashboard for the `aid stats` command.
// Exports: run(). Deps: crate::store::Store, crate::types, crate::usage, crate::cost.

use anyhow::Result;
use chrono::{DateTime, Local, Timelike};
use std::collections::{BTreeMap, HashMap};

use super::stats_hint;
use crate::cost;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskOutcome};
use crate::usage::UsageWindow;

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

fn collect(store: &Store, window: UsageWindow, agent: Option<&str>, now: DateTime<Local>) -> Result<StatsSnapshot> {
    let tasks: Vec<Task> = store.list_tasks(crate::types::TaskFilter::All)?.into_iter().filter(|t| matches!(window.range(now), Some((s, e)) if t.created_at >= s && t.created_at <= e) || matches!(window, UsageWindow::All)).filter(|t| agent.is_none_or(|name| t.agent_display_name().eq_ignore_ascii_case(name))).collect();
    let mut agents: BTreeMap<String, (AgentKind, usize, usize, usize, i64, usize, Option<f64>)> = BTreeMap::new();
    let mut failures: HashMap<String, (usize, BTreeMap<String, usize>)> = HashMap::new();
    let mut models: BTreeMap<String, (usize, Option<f64>, AgentKind)> = BTreeMap::new();
    let mut declared: BTreeMap<String, (usize, i64, usize, usize)> = BTreeMap::new();
    let (mut day_counts, mut hour_counts, mut total_cost, mut total_tokens, mut total_tasks) = (HashMap::new(), [0usize; 24], None, 0, 0);
    let (mut longest, mut most_tokens, mut highest_cost) = (None, None, None);
    for task in &tasks {
        let cost_usd = task_cost(task);
        let row = agents.entry(task.agent_display_name().to_string()).or_insert((task.agent, 0, 0, 0, 0, 0, None));
        row.1 += 1;
        let outcome = task.outcome();
        row.2 += usize::from(outcome.is_success());
        row.3 += usize::from(!matches!(outcome, TaskOutcome::InProgress | TaskOutcome::Stopped));
        if let Some(ms) = task.duration_ms { row.4 += ms; row.5 += 1; }
        add_known_cost(&mut row.6, cost_usd);
        let model = task.attributed_model().unwrap_or("unknown").to_string();
        let model_row = models.entry(model).or_insert((0, None, task.agent));
        model_row.0 += 1;
        add_known_cost(&mut model_row.1, cost_usd);
        if let Some(difficulty) = store.get_task_profile(task.id.as_str())?.difficulty {
            let row = declared.entry(difficulty.label().to_string()).or_default();
            row.0 += 1;
            if let Some(duration) = task.duration_ms { row.1 += duration; row.2 += 1; }
            row.3 += usize::from(matches!(outcome, TaskOutcome::Broken | TaskOutcome::Failed));
        }
        *day_counts.entry(task.created_at.format("%a").to_string()).or_default() += 1;
        hour_counts[task.created_at.hour() as usize] += 1;
        add_known_cost(&mut total_cost, cost_usd);
        total_tokens += task.tokens.unwrap_or(0);
        total_tasks += 1;
        if let Some(ms) = task.duration_ms { if longest.as_ref().is_none_or(|(_, best)| ms > *best) { longest = Some((task, ms)); } }
        let tokens = task.tokens.unwrap_or(0);
        if most_tokens.as_ref().is_none_or(|(_, best)| tokens > *best) { most_tokens = Some((task, tokens)); }
        if let Some(cost_usd) = cost_usd {
            if highest_cost.as_ref().is_none_or(|(_, best)| cost_usd > *best) { highest_cost = Some((task, cost_usd)); }
        }
        if matches!(outcome, TaskOutcome::Broken | TaskOutcome::Failed) {
            let label = classify_failure(store.latest_error(task.id.as_str()).as_deref(), task.exit_code);
            let entry = failures.entry(label).or_insert((0, BTreeMap::new()));
            entry.0 += 1;
            *entry.1.entry(task.agent_display_name().to_string()).or_default() += 1;
        }
    }
    let mut agent_rows: Vec<_> = agents.into_iter().map(|(agent, (kind, tasks, success, success_base, duration_ms, duration_count, cost_usd))| AgentRow {
        agent, tasks, share_pct: usage_share(tasks, total_tasks), success_rate: if success_base == 0 { 0.0 } else { success as f64 * 100.0 / success_base as f64 }, avg_duration_ms: (duration_count > 0).then(|| duration_ms / duration_count as i64), cost: cost::format_cost_label(cost_usd, kind),
    }).collect();
    agent_rows.sort_by(|a, b| b.tasks.cmp(&a.tasks).then_with(|| a.agent.cmp(&b.agent)));
    let mut failure_rows: Vec<_> = failures.into_iter().map(|(label, (tasks, agents))| {
        let mut agents: Vec<_> = agents.into_iter().collect();
        agents.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        FailureRow { label, tasks, agents }
    }).collect();
    failure_rows.sort_by(|a, b| b.tasks.cmp(&a.tasks).then_with(|| a.label.cmp(&b.label)));
    failure_rows.truncate(5);
    let mut model_rows: Vec<_> = models.into_iter().map(|(model, (tasks, cost_usd, kind))| ModelRow {
        model, tasks, cost: cost::format_cost_label(cost_usd, kind),
    }).collect();
    model_rows.sort_by(|a, b| b.tasks.cmp(&a.tasks).then_with(|| a.model.cmp(&b.model)));
    let declared_rows = declared.into_iter().map(|(difficulty, (tasks, duration, count, failures))| DeclaredRow {
        difficulty, tasks, avg_duration_ms: (count > 0).then(|| duration / count as i64), failures,
    }).collect();
    Ok(StatsSnapshot {
        agent_rows, failure_rows, model_rows, declared_rows, total_cost, total_tokens, total_tasks,
        activity_by_day: ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].into_iter().map(|day| (day.to_string(), *day_counts.get(day).unwrap_or(&0))).collect(),
        activity_by_hour: hour_counts.into_iter().enumerate().map(|(hour, count)| (hour as u32, count)).collect(),
        top_sessions: [
            longest.map(|(task, ms)| TopSession { task_id: task.id.to_string(), agent: task.display_route(), label: "Longest", value: format_duration(Some(ms)) }),
            most_tokens.map(|(task, tokens)| TopSession { task_id: task.id.to_string(), agent: task.display_route(), label: "Most tokens", value: format_tokens(tokens) }),
            highest_cost.map(|(task, cost_usd)| TopSession { task_id: task.id.to_string(), agent: task.display_route(), label: "Highest cost", value: cost::format_cost(Some(cost_usd)) }),
        ].into_iter().flatten().collect(),
    })
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

fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 { format!("{:.1}M", tokens as f64 / 1_000_000.0) } else if tokens >= 1_000 { format!("{:.1}k", tokens as f64 / 1_000.0) } else { tokens.to_string() }
}

fn usage_share(tasks: usize, total_tasks: usize) -> usize {
    (tasks * 100).checked_div(total_tasks).unwrap_or(0)
}

fn add_known_cost(total: &mut Option<f64>, cost: Option<f64>) {
    if let Some(cost) = cost {
        *total.get_or_insert(0.0) += cost;
    }
}

fn task_cost(task: &Task) -> Option<f64> {
    if let Some(cost) = task.cost_usd {
        return Some(cost);
    }
    if matches!(task.agent, AgentKind::Cursor | AgentKind::Copilot) {
        return Some(0.0);
    }
    cost::estimate_cost(task.tokens.unwrap_or(0), task.costing_model(), task.agent)
}

#[cfg(test)]
#[path = "stats_tests.rs"]
mod tests;
