// Dashboard card rendering for the aid TUI.
// Exports the checklist-style dashboard view; depends on ratatui and App state.
use super::app::App;
use super::metrics::ProcessMetrics;
use crate::types::{EventKind, Task, TaskStatus};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::{Alignment, Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
const FOOTER_HINT: &str = "a=all/today s=stats d=dashboard m=multipane j/k=nav Enter=detail q=quit";
const ACTIVITY_KINDS: &[(EventKind, &str, &str)] = &[
    (EventKind::ToolCall, "tool call", "tool calls"),
    (EventKind::Build, "build", "builds"),
    (EventKind::Test, "test", "tests"),
    (EventKind::FileWrite, "file write", "file writes"),
    (EventKind::FileRead, "file read", "file reads"),
    (EventKind::WebSearch, "web search", "web searches"),
    (EventKind::Lint, "lint", "lints"),
    (EventKind::Format, "format", "formats"),
    (EventKind::Commit, "commit", "commits"),
    (EventKind::Completion, "completion", "completions"),
    (EventKind::Error, "error", "errors"),
];
pub fn render_dashboard(frame: &mut ratatui::Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .split(frame.area());
    frame.render_widget(
        Paragraph::new(ratatui::text::Line::from(vec![
            ratatui::text::Span::styled("aid ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ratatui::text::Span::styled(
                format!("dashboard [{}]", app.scope_label()),
                Style::default().fg(Color::Indexed(250)),
            ),
        ]))
        .alignment(Alignment::Center),
        chunks[0],
    );
    if app.tasks.is_empty() {
        frame.render_widget(
            Paragraph::new(app.empty_message())
                .block(Block::default().title("Tasks").borders(Borders::ALL)),
            chunks[1],
        );
    } else {
        render_cards(frame, app, chunks[1]);
    }
    let done = app
        .tasks
        .iter()
        .filter(|task| matches!(task.status, TaskStatus::Done | TaskStatus::Merged))
        .count();
    let running = app
        .tasks
        .iter()
        .filter(|task| matches!(task.status, TaskStatus::Running | TaskStatus::AwaitingInput))
        .count();
    let footer_line = ratatui::text::Line::from(vec![
        ratatui::text::Span::styled(
            format!(" {} tasks ", app.tasks.len()),
            Style::default().fg(Color::Indexed(250)),
        ),
        ratatui::text::Span::styled(format!("{}✓ ", done), Style::default().fg(Color::Green)),
        ratatui::text::Span::styled(format!("{}▶ ", running), Style::default().fg(Color::Yellow)),
        ratatui::text::Span::styled(
            format!("│ {FOOTER_HINT}"),
            Style::default().fg(Color::Indexed(243)),
        ),
    ]);
    frame.render_widget(Paragraph::new(footer_line), chunks[2]);
}
fn render_cards(frame: &mut ratatui::Frame<'_>, app: &App, area: Rect) {
    let (start, end) = visible_task_indices(app, area.height);
    let constraints = app.tasks[start..end]
        .iter()
        .map(|task| Constraint::Length(card_height(&app.task_milestones(task.id.as_str()))))
        .collect::<Vec<_>>();
    let task_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);
    for (index, (task, task_area)) in app.tasks[start..end]
        .iter()
        .zip(task_areas.iter())
        .enumerate()
    {
        let selected = start + index == app.selected;
        let milestones = app.task_milestones(task.id.as_str());
        let activity = task_activity_summary(app, task.id.as_str());
        frame.render_widget(
            render_task_card(
                task,
                milestones,
                app.get_metrics(task.id.as_str()).copied(),
                activity,
                selected,
            ),
            *task_area,
        );
    }
}
pub fn render_task_card(
    task: &Task,
    milestones: Vec<String>,
    metrics: Option<ProcessMetrics>,
    activity: String,
    selected: bool,
) -> List<'static> {
    let cpu = metrics
        .map(|value| format!("{:.1}%", value.cpu_percent))
        .unwrap_or_else(|| "—".to_string());
    let memory = metrics
        .map(|value| format!("{:.0}M", value.memory_mb))
        .unwrap_or_else(|| "—".to_string());
    let mut items = vec![
        ListItem::new(format!("Prompt: {}", truncate(&task.prompt, 56))),
        ListItem::new(format!(
            "System: CPU {cpu}  MEM {memory}  Status {}",
            task.status.label()
        )),
        ListItem::new(format!("Activity: {}", truncate(&activity, 56))),
    ];
    let active = matches!(task.status, TaskStatus::Running | TaskStatus::AwaitingInput);
    let last = milestones.len().saturating_sub(1);
    if milestones.is_empty() {
        items.push(ListItem::new("Progress: no milestones yet"));
    } else {
        for (index, milestone) in milestones.iter().enumerate() {
            let current = active && index == last;
            let marker = if current { "[ ]" } else { "[x]" };
            let suffix = if current { " <- current" } else { "" };
            items.push(ListItem::new(format!(
                "{marker} {}{suffix}",
                truncate(milestone, 52)
            )));
        }
    }
    let mut style = status_style(task.status);
    if selected {
        style = style.bg(Color::Indexed(237)).add_modifier(Modifier::BOLD);
    }
    List::new(items)
        .block(
            Block::default()
                .title(format!(
                    " {} {} {} {} ",
                    task.id,
                    task.agent_display_name(),
                    task.status.label(),
                    milestone_progress(task, milestones.len())
                ))
                .borders(Borders::ALL)
                .border_style(style),
        )
        .style(style)
}
fn visible_task_indices(app: &App, height: u16) -> (usize, usize) {
    if app.tasks.is_empty() || height == 0 {
        return (0, 0);
    }
    let mut start = app.selected.min(app.tasks.len() - 1);
    let mut used = card_height(&app.task_milestones(app.tasks[start].id.as_str()));
    while start > 0 {
        let next = card_height(&app.task_milestones(app.tasks[start - 1].id.as_str()));
        if used + next > height {
            break;
        }
        start -= 1;
        used += next;
    }
    let mut end = start;
    let mut filled = 0;
    while end < app.tasks.len() {
        let next = card_height(&app.task_milestones(app.tasks[end].id.as_str()));
        if end > start && filled + next > height {
            break;
        }
        filled += next;
        end += 1;
    }
    (start, end)
}
fn task_activity_summary(app: &App, task_id: &str) -> String {
    let Some(events) = app.events_cache.get(task_id) else {
        return "no cached events".to_string();
    };
    let parts = ACTIVITY_KINDS
        .iter()
        .filter_map(|(kind, singular, plural)| {
            let count = events
                .iter()
                .filter(|event| event.event_kind == *kind)
                .count();
            (count > 0).then(|| format!("{count} {}", if count == 1 { *singular } else { *plural }))
        })
        .collect::<Vec<_>>();
    if parts.is_empty() {
        "no activity yet".to_string()
    } else {
        parts.join(", ")
    }
}
fn card_height(milestones: &[String]) -> u16 {
    5 + milestones.len().max(1) as u16
}
fn milestone_progress(task: &Task, count: usize) -> String {
    if count == 0 {
        "0/0".to_string()
    } else if matches!(task.status, TaskStatus::Running | TaskStatus::AwaitingInput) {
        format!("{}/{}", count.saturating_sub(1), count)
    } else {
        format!("{count}/{count}")
    }
}
fn status_style(status: TaskStatus) -> Style {
    match status {
        TaskStatus::Done | TaskStatus::Merged => Style::default().fg(Color::Green),
        TaskStatus::Running => Style::default().fg(Color::Yellow),
        TaskStatus::AwaitingInput => Style::default().fg(Color::Magenta),
        TaskStatus::Failed => Style::default().fg(Color::Red),
        TaskStatus::Pending => Style::default().fg(Color::Gray),
        TaskStatus::Skipped => Style::default().fg(Color::Blue),
    }
}
fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        value.to_string()
    } else {
        format!("{}...", &value[..max.saturating_sub(3)])
    }
}
