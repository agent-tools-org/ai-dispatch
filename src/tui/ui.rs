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
use super::dashboard;
use super::multipane;
use crate::cost;
use crate::types::{Task, TaskStatus};

pub fn render(frame: &mut ratatui::Frame<'_>, app: &App) {
    if app.multipane_mode {
        render_multipane_view(frame, app);
    } else if app.detail_mode {
        render_detail(frame, app);
    } else if app.dashboard_mode {
        dashboard::render_dashboard(frame, app);
    } else {
        render_board(frame, app);
    }
}

fn render_multipane_view(frame: &mut ratatui::Frame<'_>, app: &App) {
    let tasks = app.multipane_tasks();
    let panes: Vec<multipane::PaneData> = tasks
        .iter()
        .enumerate()
        .map(|(index, task)| {
            let events_raw = app
                .events_cache
                .get(task.id.as_str())
                .cloned()
                .unwrap_or_default();
            let total_events = events_raw.len();
            let events = events_raw
                .iter()
                .map(|e| {
                    (
                        e.timestamp.format("%H:%M:%S").to_string(),
                        e.event_kind.as_str().to_string(),
                        e.detail.clone(),
                    )
                })
                .collect();
            let elapsed = if let Some(ms) = task.duration_ms {
                // Completed task: show final duration
                let secs = ms / 1000;
                if secs < 60 {
                    format!("{secs}s")
                } else if secs < 3600 {
                    format!("{}m {:02}s", secs / 60, secs % 60)
                } else {
                    format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
                }
            } else {
                // Running task: show live elapsed
                let secs = (chrono::Local::now() - task.created_at).num_seconds();
                if secs < 60 {
                    format!("{secs}s")
                } else if secs < 3600 {
                    format!("{}m {:02}s", secs / 60, secs % 60)
                } else {
                    format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
                }
            };
            multipane::PaneData {
                task_id: task.id.as_str().to_string(),
                agent: task.agent.to_string(),
                status: task.status.label().to_string(),
                prompt: task.prompt.clone(),
                events,
                tokens: task_tokens(task),
                cost: cost::format_cost(task.cost_usd),
                model: task.model.as_deref().unwrap_or("-").to_string(),
                milestone: app.get_milestone(task.id.as_str()).unwrap_or("").to_string(),
                cpu: task_cpu(app, task),
                memory: task_memory(app, task),
                workgroup: task.workgroup_id.clone().unwrap_or_default(),
                worktree_branch: task.worktree_branch.clone().unwrap_or_default(),
                elapsed,
                scroll_offset: app.pane_scroll_offsets.get(index).copied().unwrap_or(0),
                total_events,
            }
        })
        .collect();
    multipane::render_multipane(frame, &panes, app.active_pane);
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
        "ID", "Agent", "Status", "Progress", "CPU", "Mem", "Duration", "Tokens", "Cost", "Model", "Group",
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
            Constraint::Length(32),
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

    let done = app.tasks.iter().filter(|task| matches!(task.status, TaskStatus::Done | TaskStatus::Merged)).count();
    let running = app.tasks.iter().filter(|task| matches!(task.status, TaskStatus::Running | TaskStatus::AwaitingInput)).count();
    let failed = app.tasks.iter().filter(|task| matches!(task.status, TaskStatus::Failed)).count();
    let status = format!(
        "Scope: {} | Tasks: {} | Done: {} | Running: {} | Failed: {} | d=dashboard m=multipane j/k=nav Enter=detail q=quit",
        app.scope_label(),
        app.tasks.len(),
        done,
        running,
        failed,
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
        let events = app.selected_events();
        frame.render_widget(task_header(task, &events), chunks[0]);
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
    let status = match task.status {
        TaskStatus::Running => format!("▶ {}", task.status.label()),
        TaskStatus::Done | TaskStatus::Merged => format!("✓ {}", task.status.label()),
        TaskStatus::Failed => format!("✗ {}", task.status.label()),
        _ => task.status.label().to_string(),
    };
    Row::new(vec![
        Cell::from(task.id.as_str().to_string()),
        Cell::from(task.agent.as_str().to_string()),
        Cell::from(status),
        Cell::from(task_progress(app, task)),
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

fn task_header(task: &Task, events: &[crate::types::TaskEvent]) -> Paragraph<'static> {
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
    if task.status == TaskStatus::AwaitingInput
        && let Some(prompt) = pending_prompt(events)
    {
        lines.push(format!("Awaiting: {}", truncate(prompt, 120)));
    }
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

fn task_progress(app: &App, task: &Task) -> String {
    if task.status == TaskStatus::AwaitingInput {
        return "awaiting input".to_string();
    }
    if task.status != TaskStatus::Running {
        return "—".to_string();
    }
    app.get_milestone(task.id.as_str())
        .map(|milestone| truncate(milestone, 30))
        .unwrap_or_else(|| "—".to_string())
}

fn status_style(status: TaskStatus) -> Style {
    match status {
        TaskStatus::Done | TaskStatus::Merged => Style::default().fg(Color::DarkGray),
        TaskStatus::Running => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        TaskStatus::AwaitingInput => Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        TaskStatus::Failed => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        TaskStatus::Pending => Style::default().fg(Color::Gray),
        TaskStatus::Skipped => Style::default().fg(Color::Blue),
    }
}

fn pending_prompt(events: &[crate::types::TaskEvent]) -> Option<&str> {
    events.iter().rev().find_map(|event| {
        let metadata = event.metadata.as_ref()?;
        metadata
            .get("awaiting_input")
            .and_then(|value| value.as_bool())
            .filter(|value| *value)
            .map(|_| event.detail.as_str())
    })
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        value.to_string()
    } else {
        format!("{}...", &value[..max.saturating_sub(3)])
    }
}
