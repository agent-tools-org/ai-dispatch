// Legacy agent statistics widgets for the aid TUI.
// Exports: cost chart, success chart, budget gauges, and legacy summary.
// Deps: App task/config state, cost formatting, and ratatui chart widgets.

use super::app::App;
use crate::cost;
use crate::types::{AgentKind, Task, TaskOutcome};
use chrono::{Duration, Local};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Bar, BarChart, BarGroup, Block, Borders, Gauge, Paragraph, Sparkline};

const AGENTS: &[AgentKind] = AgentKind::ALL_BUILTIN;

struct BudgetUsage {
    name: String,
    used: f64,
    limit: f64,
}

pub(crate) fn render_legacy_grid(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[0]);
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);
    render_cost_chart(frame, app, top[0]);
    render_success_chart(frame, app, top[1]);
    render_budget_gauges(frame, app, bottom[0]);
    render_legacy_summary(frame, app, bottom[1]);
}

fn render_cost_chart(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let mut max = 1;
    let bars = AGENTS
        .iter()
        .map(|agent| {
            let cents = (app.tasks.iter().filter(|task| task.agent == *agent).filter_map(|task| task.cost_usd).sum::<f64>() * 100.0) as u64;
            max = max.max(cents);
            Bar::default()
                .label(Line::from(agent.as_str()))
                .value(cents)
                .style(agent_color(*agent))
                .text_value(cost::format_cost(Some(cents as f64 / 100.0)))
        })
        .collect::<Vec<_>>();
    let chart = BarChart::default()
        .block(Block::default().title("Cost by Agent · tasks.cost_usd").borders(Borders::ALL))
        .direction(Direction::Horizontal)
        .bar_gap(0)
        .data(BarGroup::default().bars(&bars))
        .max(max)
        .value_style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD));
    frame.render_widget(chart, area);
}

fn render_success_chart(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let bars = AGENTS
        .iter()
        .map(|agent| {
            let total = app.tasks.iter().filter(|task| task.agent == *agent).filter(|task| task.outcome() != TaskOutcome::Stopped).count();
            let success = app.tasks.iter().filter(|task| task.agent == *agent).filter(|task| task.outcome().is_success()).count();
            let rate = (success * 100).checked_div(total).unwrap_or(0) as u64;
            Bar::default()
                .label(Line::from(agent.as_str()))
                .value(rate)
                .style(success_color(rate))
                .text_value(format!("{rate}%"))
        })
        .collect::<Vec<_>>();
    let chart = BarChart::default()
        .block(Block::default().title("Success Rate · task outcome").borders(Borders::ALL))
        .direction(Direction::Horizontal)
        .bar_gap(0)
        .data(BarGroup::default().bars(&bars))
        .max(100)
        .value_style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD));
    frame.render_widget(chart, area);
}

fn render_budget_gauges(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let block = Block::default().title("Budget Usage · tasks.cost_usd + config").borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let usage = budget_usage(app);
    if usage.is_empty() || inner.is_empty() {
        frame.render_widget(Paragraph::new(if usage.is_empty() { "No budgets configured. See `aid config`." } else { "" }), inner);
        return;
    }
    let visible = usage.len().min(inner.height as usize);
    let rows = Layout::default().direction(Direction::Vertical).constraints(vec![Constraint::Length(1); visible]).split(inner);
    for (budget, row) in usage.iter().take(visible).zip(rows.iter()) {
        let ratio = if budget.limit <= 0.0 { 0.0 } else { (budget.used / budget.limit).max(0.0) };
        let label = format!("{}: {}/{} ({}%)", budget.name, cost::format_cost(Some(budget.used)), cost::format_cost(Some(budget.limit)), (ratio * 100.0).round() as u64);
        let gauge = Gauge::default().ratio(ratio.clamp(0.0, 1.0)).label(label).use_unicode(true).gauge_style(Style::default().fg(gauge_color(ratio)));
        frame.render_widget(gauge, *row);
    }
}

fn render_legacy_summary(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let block = Block::default().title("Summary · task table").borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.is_empty() { return; }
    let parts = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(4), Constraint::Min(1)]).split(inner);
    let done = app.tasks.iter().filter(|task| task.outcome().is_success()).count();
    let failed = app.tasks.iter().filter(|task| matches!(task.outcome(), TaskOutcome::Broken | TaskOutcome::Failed)).count();
    let running = app.tasks.iter().filter(|task| matches!(task.outcome(), TaskOutcome::InProgress)).count();
    let total_cost = app.tasks.iter().filter_map(|task| task.cost_usd).sum::<f64>();
    let today = Local::now().date_naive();
    let today_cost = app.tasks.iter().filter(|task| task.created_at.date_naive() == today).filter_map(|task| task.cost_usd).sum::<f64>();
    let total_tokens = app.tasks.iter().filter_map(|task| task.tokens).sum::<i64>();
    let summary = vec![
        Line::from(format!("Tasks: {}  Done: {done}  Failed: {failed}  Running: {running}", app.tasks.len())),
        Line::from(format!("Cost: {} total  {} today", cost::format_cost(Some(total_cost)), cost::format_cost(Some(today_cost)))),
        Line::from(format!("Tokens: {}", format_tokens(total_tokens))),
        Line::from("Recent task cost · tasks.cost_usd"),
    ];
    let spark = recent_costs(app);
    let max = spark.iter().copied().max().unwrap_or(1).max(1);
    frame.render_widget(Paragraph::new(summary), parts[0]);
    frame.render_widget(Sparkline::default().data(spark).max(max).style(Style::default().fg(Color::Cyan)), parts[1]);
}

fn budget_usage(app: &App) -> Vec<BudgetUsage> {
    app.config().usage.budgets.iter().filter_map(|budget| {
        let limit = budget.cost_limit_usd?;
        let used = filter_budget_tasks(&app.tasks, budget.agent.as_deref(), budget.window.as_deref()).into_iter().filter_map(|task| task.cost_usd).sum::<f64>() + budget.external_cost_usd;
        Some(BudgetUsage { name: budget.name.clone(), used, limit })
    }).collect()
}

fn filter_budget_tasks<'a>(tasks: &'a [Task], agent: Option<&str>, window: Option<&str>) -> Vec<&'a Task> {
    let window_start = window.and_then(parse_window).map(|value| Local::now() - value);
    tasks.iter().filter(|task| agent.map(|name| task.agent_display_name() == name).unwrap_or(false)).filter(|task| window_start.map(|start| task.created_at >= start).unwrap_or(true)).collect()
}

fn parse_window(value: &str) -> Option<Duration> {
    let trimmed = value.trim();
    if let Some(hours) = trimmed.strip_suffix('h') { return hours.parse::<i64>().ok().map(Duration::hours); }
    if let Some(days) = trimmed.strip_suffix('d') { return days.parse::<i64>().ok().map(Duration::days); }
    trimmed.strip_suffix('m').and_then(|minutes| minutes.parse::<i64>().ok().map(Duration::minutes))
}

fn recent_costs(app: &App) -> Vec<u64> {
    let values = app.tasks.iter().rev().take(20).filter_map(|task| task.cost_usd.map(|cost| (cost * 100.0) as u64)).collect::<Vec<_>>();
    if values.is_empty() { vec![0] } else { values }
}

fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 { format!("{:.1}M", tokens as f64 / 1_000_000.0) } else if tokens >= 1_000 { format!("{:.1}k", tokens as f64 / 1_000.0) } else { tokens.to_string() }
}

fn agent_color(agent: AgentKind) -> Color {
    match agent {
        AgentKind::Codex => Color::Cyan, AgentKind::Gemini => Color::Green, AgentKind::Antigravity => Color::LightYellow,
        AgentKind::Qwen => Color::LightRed, AgentKind::Copilot => Color::LightGreen, AgentKind::OpenCode => Color::Yellow,
        AgentKind::CommandCode => Color::LightYellow, AgentKind::Cursor => Color::Magenta, AgentKind::Kilo => Color::Blue,
        AgentKind::MiMoCode => Color::Blue, AgentKind::Droid => Color::LightMagenta, AgentKind::Oz => Color::LightBlue,
        AgentKind::Claude => Color::White, AgentKind::Grok => Color::Rgb(255, 107, 53), AgentKind::Custom => Color::Gray,
    }
}

fn success_color(rate: u64) -> Color { if rate >= 80 { Color::Green } else if rate >= 50 { Color::Yellow } else { Color::Red } }

fn gauge_color(ratio: f64) -> Color { if ratio > 0.8 { Color::Red } else if ratio >= 0.5 { Color::Yellow } else { Color::Green } }
