// Stats and charts view for the aid TUI.
// Renders agent cost bars, success rates, budget gauges, and cost sparkline.
// Deps: ratatui widgets (BarChart, Sparkline, Gauge), App state, usage module.

use super::app::App;
use crate::cost;
use crate::types::{AgentKind, Task, TaskStatus};
use chrono::{Duration, Local};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::prelude::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Bar, BarChart, BarGroup, Block, Borders, Gauge, Paragraph, Sparkline};
const AGENTS: &[AgentKind] = AgentKind::ALL_BUILTIN;
const FOOTER_HINT: &str = "a=all/today s=stats d=dashboard m=multipane q=quit";
struct BudgetUsage {
    name: String,
    used: f64,
    limit: f64,
}

pub fn render_stats(frame: &mut ratatui::Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Percentage(40),
            Constraint::Percentage(40),
            Constraint::Min(1),
        ])
        .split(frame.area());
    let top_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);
    let bottom_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[2]);
    render_title(frame, app, chunks[0]);
    render_cost_chart(frame, app, top_cols[0]);
    render_success_chart(frame, app, top_cols[1]);
    render_budget_gauges(frame, app, bottom_cols[0]);
    render_summary(frame, app, bottom_cols[1]);
    render_footer(frame, chunks[3]);
}
fn render_title(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let title = Line::from(vec![
        Span::styled(
            "aid ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("stats [{}]", app.scope_label()),
            Style::default().fg(Color::Indexed(250)),
        ),
    ]);
    frame.render_widget(Paragraph::new(title).alignment(Alignment::Center), area);
}
fn render_cost_chart(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let mut max = 1;
    let bars = AGENTS
        .iter()
        .map(|agent| {
            let cents = app
                .tasks
                .iter()
                .filter(|task| task.agent == *agent)
                .filter_map(|task| task.cost_usd)
                .sum::<f64>()
                * 100.0;
            let cents = cents as u64;
            max = max.max(cents);
            Bar::default()
                .label(Line::from(agent.as_str()))
                .value(cents)
                .style(agent_color(*agent))
                .text_value(cost::format_cost(Some(cents as f64 / 100.0)))
        })
        .collect::<Vec<_>>();
    let chart = BarChart::default()
        .block(
            Block::default()
                .title("Cost by Agent")
                .borders(Borders::ALL),
        )
        .direction(Direction::Horizontal)
        .bar_gap(0)
        .data(BarGroup::default().bars(&bars))
        .max(max)
        .value_style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(chart, area);
}
fn render_success_chart(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let bars = AGENTS
        .iter()
        .map(|agent| {
            let total = app.tasks.iter().filter(|task| task.agent == *agent).count();
            let success = app
                .tasks
                .iter()
                .filter(|task| task.agent == *agent)
                .filter(|task| matches!(task.status, TaskStatus::Done | TaskStatus::Merged))
                .count();
            let rate = if total == 0 {
                0
            } else {
                (success * 100 / total) as u64
            };
            Bar::default()
                .label(Line::from(agent.as_str()))
                .value(rate)
                .style(success_color(rate))
                .text_value(format!("{rate}%"))
        })
        .collect::<Vec<_>>();
    let chart = BarChart::default()
        .block(
            Block::default()
                .title("Success Rate (%)")
                .borders(Borders::ALL),
        )
        .direction(Direction::Horizontal)
        .bar_gap(0)
        .data(BarGroup::default().bars(&bars))
        .max(100)
        .value_style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(chart, area);
}
fn render_budget_gauges(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let block = Block::default().title("Budget Usage").borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let usage = budget_usage(app);
    if usage.is_empty() || inner.is_empty() {
        frame.render_widget(
            Paragraph::new(if usage.is_empty() {
                "No budgets configured. See `aid config`."
            } else {
                ""
            }),
            inner,
        );
        return;
    }
    let visible = usage.len().min(inner.height as usize);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Length(1); visible])
        .split(inner);
    for (budget, row) in usage.iter().take(visible).zip(rows.iter()) {
        let ratio = if budget.limit <= 0.0 {
            0.0
        } else {
            (budget.used / budget.limit).max(0.0)
        };
        let percent = (ratio * 100.0).round() as u64;
        let label = format!(
            "{}: {}/{} ({}%)",
            budget.name,
            cost::format_cost(Some(budget.used)),
            cost::format_cost(Some(budget.limit)),
            percent
        );
        let gauge = Gauge::default()
            .ratio(ratio.clamp(0.0, 1.0))
            .label(label)
            .use_unicode(true)
            .gauge_style(Style::default().fg(gauge_color(ratio)));
        frame.render_widget(gauge, *row);
    }
}
fn render_summary(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let block = Block::default().title("Summary").borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.is_empty() {
        return;
    }
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(1)])
        .split(inner);
    let done = app
        .tasks
        .iter()
        .filter(|task| matches!(task.status, TaskStatus::Done | TaskStatus::Merged))
        .count();
    let failed = app
        .tasks
        .iter()
        .filter(|task| matches!(task.status, TaskStatus::Failed))
        .count();
    let running = app
        .tasks
        .iter()
        .filter(|task| matches!(task.status, TaskStatus::Running | TaskStatus::AwaitingInput))
        .count();
    let total_cost = app
        .tasks
        .iter()
        .filter_map(|task| task.cost_usd)
        .sum::<f64>();
    let today = Local::now().date_naive();
    let today_cost = app
        .tasks
        .iter()
        .filter(|task| task.created_at.date_naive() == today)
        .filter_map(|task| task.cost_usd)
        .sum::<f64>();
    let total_tokens = app.tasks.iter().filter_map(|task| task.tokens).sum::<i64>();
    let summary = vec![
        Line::from(format!(
            "Tasks: {}  Done: {}  Failed: {}  Running: {}",
            app.tasks.len(),
            done,
            failed,
            running
        )),
        Line::from(format!(
            "Cost: {} total  {} today",
            cost::format_cost(Some(total_cost)),
            cost::format_cost(Some(today_cost))
        )),
        Line::from(format!("Tokens: {}", format_tokens(total_tokens))),
        Line::from("Recent task cost"),
    ];
    let spark = recent_costs(app);
    let max = spark.iter().copied().max().unwrap_or(1).max(1);
    frame.render_widget(Paragraph::new(summary), parts[0]);
    frame.render_widget(
        Sparkline::default()
            .data(spark)
            .max(max)
            .style(Style::default().fg(Color::Cyan)),
        parts[1],
    );
}
fn render_footer(frame: &mut ratatui::Frame<'_>, area: Rect) {
    frame.render_widget(
        Paragraph::new(FOOTER_HINT).style(Style::default().fg(Color::Indexed(243))),
        area,
    );
}
fn budget_usage(app: &App) -> Vec<BudgetUsage> {
    app.config()
        .usage
        .budgets
        .iter()
        .filter_map(|budget| {
            let limit = budget.cost_limit_usd?;
            let used = filter_budget_tasks(
                &app.tasks,
                budget.agent.as_deref(),
                budget.window.as_deref(),
            )
            .into_iter()
            .filter_map(|task| task.cost_usd)
            .sum::<f64>()
                + budget.external_cost_usd;
            Some(BudgetUsage {
                name: budget.name.clone(),
                used,
                limit,
            })
        })
        .collect()
}
fn filter_budget_tasks<'a>(
    tasks: &'a [Task],
    agent: Option<&str>,
    window: Option<&str>,
) -> Vec<&'a Task> {
    let window_start = window
        .and_then(parse_window)
        .map(|value| Local::now() - value);
    tasks
        .iter()
        .filter(|task| {
            agent
                .map(|name| task.agent_display_name() == name)
                .unwrap_or(false)
        })
        .filter(|task| {
            window_start
                .map(|start| task.created_at >= start)
                .unwrap_or(true)
        })
        .collect()
}
fn parse_window(value: &str) -> Option<Duration> {
    let trimmed = value.trim();
    if let Some(hours) = trimmed.strip_suffix('h') {
        return hours.parse::<i64>().ok().map(Duration::hours);
    }
    if let Some(days) = trimmed.strip_suffix('d') {
        return days.parse::<i64>().ok().map(Duration::days);
    }
    trimmed
        .strip_suffix('m')
        .and_then(|minutes| minutes.parse::<i64>().ok().map(Duration::minutes))
}
fn recent_costs(app: &App) -> Vec<u64> {
    let values = app
        .tasks
        .iter()
        .rev()
        .take(20)
        .filter_map(|task| task.cost_usd.map(|cost| (cost * 100.0) as u64))
        .collect::<Vec<_>>();
    if values.is_empty() {
        vec![0]
    } else {
        values
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
fn agent_color(agent: AgentKind) -> Color {
    match agent {
        AgentKind::Codex => Color::Cyan,
        AgentKind::Gemini => Color::Green,
        AgentKind::OpenCode => Color::Yellow,
        AgentKind::Cursor => Color::Magenta,
        AgentKind::Kilo => Color::Blue,
        AgentKind::Codebuff => Color::LightCyan,
        AgentKind::Droid => Color::LightMagenta,
        AgentKind::Oz => Color::LightBlue,
        AgentKind::Claude => Color::White,
        AgentKind::Custom => Color::Gray,
    }
}
fn success_color(rate: u64) -> Color {
    if rate >= 80 {
        Color::Green
    } else if rate >= 50 {
        Color::Yellow
    } else {
        Color::Red
    }
}
fn gauge_color(ratio: f64) -> Color {
    if ratio > 0.8 {
        Color::Red
    } else if ratio >= 0.5 {
        Color::Yellow
    } else {
        Color::Green
    }
}
