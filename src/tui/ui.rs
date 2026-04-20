// ratatui rendering for the aid dashboard board and detail screens.
// Draws table/list widgets from App state with simple status coloring.

#[path = "ui_helpers.rs"]
mod ui_helpers;
#[path = "ui_detail.rs"]
mod ui_detail;
#[path = "ui_tree.rs"]
mod ui_tree;
use ui_helpers::*;

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::prelude::{Alignment, Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Paragraph, Row, Table, TableState,
};

use super::app::App;
use super::charts;
use super::dashboard;
use super::multipane;
use crate::cost;
use crate::types::TaskStatus;

pub fn render(frame: &mut ratatui::Frame<'_>, app: &App) {
    if app.tree_mode {
        ui_tree::render_tree_view(frame, app);
    } else if app.multipane_mode {
        render_multipane_view(frame, app);
    } else if app.detail_mode {
        ui_detail::render_detail(frame, app);
    } else if app.stats_mode {
        charts::render_stats(frame, app);
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
                let secs = ms / 1000;
                if secs < 60 {
                    format!("{secs}s")
                } else if secs < 3600 {
                    format!("{}m {:02}s", secs / 60, secs % 60)
                } else {
                    format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
                }
            } else {
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
                agent: task.agent_display_name().to_string(),
                status: task.status.label().to_string(),
                prompt: task.prompt.clone(),
                events,
                tokens: task_tokens(task),
                cost: cost::format_cost_label(task.cost_usd, task.agent),
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
        Paragraph::new(Line::from(vec![
            Span::styled("aid ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("[{}]", app.scope_label()),
                Style::default().fg(Color::Indexed(250)),
            ),
        ]))
        .alignment(Alignment::Center),
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
            .bg(Color::Indexed(237))
            .add_modifier(Modifier::BOLD),
    );

    let mut state = TableState::default();
    if !app.tasks.is_empty() {
        state.select(Some(app.selected));
    }
    frame.render_stateful_widget(table, chunks[1], &mut state);

    let done = app.tasks.iter().filter(|task| matches!(task.status, TaskStatus::Done | TaskStatus::Merged)).count();
    let running = app
        .tasks
        .iter()
        .filter(|task| {
            matches!(
                task.status,
                TaskStatus::Running | TaskStatus::AwaitingInput | TaskStatus::Stalled
            )
        })
        .count();
    let failed = app.tasks.iter().filter(|task| matches!(task.status, TaskStatus::Failed)).count();
    let status_line = Line::from(vec![
        Span::styled(
            format!(" {} tasks ", app.tasks.len()),
            Style::default().fg(Color::Indexed(250)),
        ),
        Span::styled(
            format!("{}✓ ", done),
            Style::default().fg(Color::Green),
        ),
        Span::styled(
            format!("{}▶ ", running),
            Style::default().fg(Color::Yellow),
        ),
        Span::styled(
            format!("{}✗ ", failed),
            Style::default().fg(Color::Red),
        ),
        Span::styled(
            "│ a=all/today s=stats d=dashboard m=multipane j/k=nav Enter=detail q=quit",
            Style::default().fg(Color::Indexed(243)),
        ),
    ]);
    frame.render_widget(Paragraph::new(status_line), chunks[2]);
}

fn status_to_color(status: TaskStatus) -> Color {
    match status {
        TaskStatus::Done | TaskStatus::Merged => Color::Green,
        TaskStatus::Failed => Color::Red,
        TaskStatus::Stopped => Color::Red,
        TaskStatus::Pending => Color::Indexed(250),
        TaskStatus::Waiting => Color::Indexed(240),
        TaskStatus::AwaitingInput => Color::Magenta,
        TaskStatus::Running => Color::Yellow,
        TaskStatus::Stalled => Color::LightRed,
        TaskStatus::Skipped => Color::Blue,
    }
}

#[cfg(test)]
mod tests {
    use super::status_to_color;
    use ratatui::prelude::Color;
    use crate::types::TaskStatus;

    #[test]
    fn status_to_color_maps_terminal_states() {
        assert_eq!(status_to_color(TaskStatus::Done), Color::Green);
        assert_eq!(status_to_color(TaskStatus::Merged), Color::Green);
        assert_eq!(status_to_color(TaskStatus::Failed), Color::Red);
        assert_eq!(status_to_color(TaskStatus::Running), Color::Yellow);
    }
}
