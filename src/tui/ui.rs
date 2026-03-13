// ratatui rendering for the aid dashboard board and detail screens.
// Draws table/list widgets from App state with simple status coloring.

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::prelude::{Alignment, Color, Modifier, Style};
use ratatui::widgets::{
    Block,
    Borders,
    Cell,
    List,
    ListItem,
    Paragraph,
    Row,
    Table,
    TableState,
};

use super::app::App;
use crate::cost;
use crate::types::{Task, TaskStatus};

pub fn render(frame: &mut ratatui::Frame<'_>, app: &App) {
    if app.detail_mode {
        render_detail(frame, app);
    } else {
        render_board(frame, app);
    }
}

fn render_board(frame: &mut ratatui::Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .split(frame.area());

    frame.render_widget(
        Paragraph::new(format!("aid dashboard [{}]", app.scope_label()))
            .alignment(Alignment::Center)
            .style(Style::default().add_modifier(Modifier::BOLD)),
        chunks[0],
    );

    let header = Row::new(vec![
        "ID", "Agent", "Status", "CPU", "Mem", "Duration", "Tokens", "Cost", "Model", "Group",
        "Prompt",
    ])
    .style(Style::default().add_modifier(Modifier::BOLD));
    let rows = app.tasks.iter().map(|task| task_row(app, task));
    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(14),
            Constraint::Length(10),
            Constraint::Min(20),
        ],
    )
    .header(header)
    .block(Block::default().title("Tasks").borders(Borders::ALL))
    .row_highlight_style(
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );

    let mut state = TableState::default();
    if !app.tasks.is_empty() {
        state.select(Some(app.selected));
    }
    frame.render_stateful_widget(table, chunks[1], &mut state);

    let done = app.tasks.iter().filter(|task| task.status == TaskStatus::Done).count();
    let running = app.tasks.iter().filter(|task| task.status == TaskStatus::Running).count();
    let status = format!(
        "Scope: {} | Tasks: {} | Done: {} | Running: {} | q=quit j/k=nav Enter=detail",
        app.scope_label(),
        app.tasks.len(),
        done,
        running,
    );
    frame.render_widget(Paragraph::new(status), chunks[2]);
}

fn render_detail(frame: &mut ratatui::Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .split(frame.area());

    if let Some(task) = app.selected_task() {
        frame.render_widget(task_header(task), chunks[0]);
        let events = app.selected_events();
        let items: Vec<ListItem<'_>> = events
            .iter()
            .map(|event| {
                ListItem::new(format!(
                    "{} [{}] {}",
                    event.timestamp.format("%H:%M:%S"),
                    event.event_kind.as_str(),
                    event.detail,
                ))
            })
            .collect();
        let list = List::new(items)
            .block(Block::default().title("Events").borders(Borders::ALL));
        frame.render_widget(list, chunks[1]);
    } else {
        frame.render_widget(
            Paragraph::new(app.empty_message())
                .block(Block::default().title("Task").borders(Borders::ALL)),
            chunks[0],
        );
    }

    frame.render_widget(Paragraph::new("Esc=back q=quit"), chunks[2]);
}

fn task_row(app: &App, task: &Task) -> Row<'static> {
    Row::new(vec![
        Cell::from(task.id.as_str().to_string()),
        Cell::from(task.agent.as_str().to_string()),
        Cell::from(task.status.label().to_string()),
        Cell::from(task_cpu(app, task)),
        Cell::from(task_memory(app, task)),
        Cell::from(task_duration(task)),
        Cell::from(task_tokens(task)),
        Cell::from(cost::format_cost(task.cost_usd)),
        Cell::from(truncate(task.model.as_deref().unwrap_or("-"), 14)),
        Cell::from(task.workgroup_id.clone().unwrap_or_else(|| "-".to_string())),
        Cell::from(truncate(&task.prompt, 60)),
    ])
    .style(status_style(task.status))
}

fn task_header(task: &Task) -> Paragraph<'static> {
    let mut lines = vec![
        format!("{}  {}  {}", task.id, task.agent, task.status.label()),
        format!("Prompt: {}", truncate(&task.prompt, 120)),
        format!(
            "Duration: {}  Tokens: {}  Cost: {}  Model: {}",
            task_duration(task),
            task_tokens(task),
            cost::format_cost(task.cost_usd),
            task.model.as_deref().unwrap_or("-"),
        ),
    ];
    if let Some(group) = task.workgroup_id.as_deref() {
        lines.push(format!("Group: {group}"));
    }
    if let Some(worktree) = task.worktree_path.as_deref() {
        lines.push(format!("Worktree: {worktree}"));
    }

    Paragraph::new(lines.join("\n"))
        .block(Block::default().title("Task").borders(Borders::ALL))
}

fn task_duration(task: &Task) -> String {
    task.duration_ms
        .map(|ms| {
            let secs = ms / 1000;
            if secs < 60 {
                format!("{secs}s")
            } else {
                format!("{}m {:02}s", secs / 60, secs % 60)
            }
        })
        .unwrap_or_else(|| "-".to_string())
}

fn task_tokens(task: &Task) -> String {
    task.tokens
        .map(|tokens| {
            if tokens >= 1_000_000 {
                format!("{:.1}M", tokens as f64 / 1_000_000.0)
            } else if tokens >= 1_000 {
                format!("{:.1}k", tokens as f64 / 1_000.0)
            } else {
                tokens.to_string()
            }
        })
        .unwrap_or_else(|| "-".to_string())
}

fn task_cpu(app: &App, task: &Task) -> String {
    app.get_metrics(task.id.as_str())
        .map(|metrics| format!("{:.1}%", metrics.cpu_percent))
        .unwrap_or_else(|| "—".to_string())
}

fn task_memory(app: &App, task: &Task) -> String {
    app.get_metrics(task.id.as_str())
        .map(|metrics| format!("{:.0}M", metrics.memory_mb))
        .unwrap_or_else(|| "—".to_string())
}

fn status_style(status: TaskStatus) -> Style {
    match status {
        TaskStatus::Done => Style::default().fg(Color::Green),
        TaskStatus::Running => Style::default().fg(Color::Yellow),
        TaskStatus::Failed => Style::default().fg(Color::Red),
        TaskStatus::Pending => Style::default().fg(Color::Gray),
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        value.to_string()
    } else {
        format!("{}...", &value[..max.saturating_sub(3)])
    }
}
