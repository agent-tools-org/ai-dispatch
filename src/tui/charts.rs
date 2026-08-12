// Statistics dashboard rendering for the aid TUI.
// Exports: render_stats; consumes refresh-time StatsSnapshot values from App.
// Deps: ratatui widgets, tui::stats aggregation, shared status bar, and legacy charts.

use super::app::App;
use super::stats::{format_duration, format_tokens, DailyStats, ProjectStats, StatsRange};
use super::stats_legacy::render_legacy_grid;
use super::status_bar::{render_status_bar, StatusBarMode};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::prelude::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Sparkline, Table};

const RANGE_LABELS: [(StatsRange, &str); 4] = [
    (StatsRange::AllTime, "All time"),
    (StatsRange::Last30Days, "Last 30 days"),
    (StatsRange::Last7Days, "Last 7 days"),
    (StatsRange::Today, "Today"),
];
const WEEKDAY_LABELS: [&str; 7] = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];

pub fn render_stats(frame: &mut ratatui::Frame<'_>, app: &App) {
    if app.legacy_stats_view {
        render_legacy_stats(frame, app);
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(4),
            Constraint::Length(9),
            Constraint::Length(6),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .split(frame.area());
    render_title(frame, app, chunks[0]);
    render_range_selector(frame, app, chunks[1]);
    render_summary(frame, app, chunks[2]);
    render_heatmap(frame, &app.stats.activity, app.stats.activity_max, app.stats.current_streak, app.stats.best_streak, chunks[3]);
    render_token_trend(frame, &app.stats.token_trend, app.stats.token_max, app.stats.peak_tokens, chunks[4]);
    render_projects(frame, &app.stats.projects, chunks[5]);
    render_status_bar(frame, chunks[6], app, StatusBarMode::Stats);
}

fn render_legacy_stats(frame: &mut ratatui::Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(8), Constraint::Length(1)])
        .split(frame.area());
    let title = Line::from(vec![
        Span::styled("aid ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(format!("legacy stats [{}]", app.scope_label()), Style::default().fg(Color::Indexed(250))),
    ]);
    frame.render_widget(Paragraph::new(title).alignment(Alignment::Center), chunks[0]);
    render_legacy_grid(frame, app, chunks[1]);
    render_status_bar(frame, chunks[2], app, StatusBarMode::Stats);
}

fn render_title(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let title = Line::from(vec![
        Span::styled("aid ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(format!("stats · {} · tasks, not sessions", app.stats_range.label()), Style::default().fg(Color::Indexed(250))),
    ]);
    frame.render_widget(Paragraph::new(title).alignment(Alignment::Center), area);
}

fn render_range_selector(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let spans = RANGE_LABELS.iter().flat_map(|(range, label)| {
        let style = if *range == app.stats_range {
            Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else { Style::default().fg(Color::Indexed(250)) };
        [Span::raw(" "), Span::styled(*label, style), Span::raw(" ")]
    }).collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(Line::from(spans))
            .block(Block::default().title("Time range · tasks.created_at").borders(Borders::ALL))
            .alignment(Alignment::Center),
        area,
    );
}

fn render_summary(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let columns = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage(33), Constraint::Percentage(34), Constraint::Percentage(33)]).split(area);
    let stats = [
        ("Tasks · tasks table", app.stats.total_tasks.to_string()),
        ("Duration · completed_at−created_at", format_known_duration(app.stats.total_duration_secs, app.stats.duration_task_count)),
        ("Tokens · tasks.tokens", format_known_tokens(app.stats.total_tokens, app.stats.token_task_count)),
    ];
    for ((title, value), column) in stats.into_iter().zip(columns.iter().copied()) {
        frame.render_widget(Paragraph::new(value).block(Block::default().title(title).borders(Borders::ALL)).alignment(Alignment::Center), column);
    }
}

fn render_heatmap(frame: &mut ratatui::Frame<'_>, days: &[DailyStats], max: usize, streak: usize, best_streak: usize, area: Rect) {
    let title = format!("Activity by day · tasks.created_at · streak {streak}d · best {best_streak}d");
    let lines = WEEKDAY_LABELS.iter().enumerate().map(|(weekday, label)| {
        let mut spans = vec![Span::styled(format!("{label} "), Style::default().fg(Color::Indexed(243)))];
        spans.extend(days.iter().filter(|day| day.weekday == weekday as u32).map(|day| Span::styled(heat_cell(day.task_count, max), heat_style(day.task_count, max))));
        Line::from(spans)
    }).collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines).block(Block::default().title(title).borders(Borders::ALL)), area);
}

fn render_token_trend(frame: &mut ratatui::Frame<'_>, data: &[u64], max: u64, peak: Option<(chrono::NaiveDate, i64)>, area: Rect) {
    let peak_label = peak.map(|(date, tokens)| format!("peak {} · {}", date.format("%Y-%m-%d"), format_tokens(tokens))).unwrap_or_else(|| "peak unavailable · no recorded tokens".to_string());
    let empty_data = [0_u64];
    let spark_data = if data.is_empty() { &empty_data[..] } else { data };
    frame.render_widget(Sparkline::default().block(Block::default().title(format!("Token trend · tasks.tokens · {peak_label}")).borders(Borders::ALL)).data(spark_data).max(max).style(Style::default().fg(Color::Cyan)), area);
}

fn render_projects(frame: &mut ratatui::Frame<'_>, projects: &[ProjectStats], area: Rect) {
    let rows = projects.iter().map(|project| Row::new([
        project.name.clone(), project.task_count.to_string(), format_known_tokens(project.tokens, project.token_task_count), format_known_duration(project.duration_secs, project.duration_task_count),
    ]));
    let table = Table::new(rows, [Constraint::Min(20), Constraint::Length(8), Constraint::Length(16), Constraint::Length(16)])
        .header(Row::new(["Project · repo_path", "Tasks", "Tokens · tasks.tokens", "Duration · completed_at−created_at"]).style(Style::default().add_modifier(Modifier::BOLD)))
        .block(Block::default().title("Projects · ranked by task count, then tokens").borders(Borders::ALL));
    frame.render_widget(table, area);
}

fn format_known_tokens(tokens: i64, recorded_tasks: usize) -> String {
    if recorded_tasks == 0 { "—".to_string() } else { format!("{} ({recorded_tasks} recorded)", format_tokens(tokens)) }
}

fn format_known_duration(seconds: i64, measured_tasks: usize) -> String {
    if measured_tasks == 0 { "—".to_string() } else { format!("{} ({measured_tasks} complete)", format_duration(seconds)) }
}

fn heat_cell(count: usize, max: usize) -> String {
    let symbol = match (count, max) {
        (0, _) | (_, 0) => '·',
        (count, max) if count * 4 >= max * 3 => '█',
        (count, max) if count * 2 >= max => '▓',
        (count, max) if count * 4 >= max => '▒',
        _ => '░',
    };
    symbol.to_string()
}

fn heat_style(count: usize, max: usize) -> Style {
    if count == 0 || max == 0 { Style::default().fg(Color::Indexed(238)) } else { Style::default().fg(Color::Green) }
}
