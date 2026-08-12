// ratatui rendering for the aid dashboard board and detail screens.
// Draws table/list widgets from App state with simple status coloring.

#[path = "ui_helpers.rs"]
pub(crate) mod ui_helpers;
#[path = "ui_detail.rs"]
mod ui_detail;
#[path = "ui_tree.rs"]
mod ui_tree;
use ui_helpers::*;

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::prelude::{Alignment, Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, Paragraph, Row, Table, TableState,
};
use super::app::App;
use super::charts;
use super::dashboard;
use super::multipane;
use super::status_bar::{render_status_bar, StatusBarMode};
use crate::cost;
use crate::types::TaskStatus;
use crate::tui::tree_data::{self, TreeNode};

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
        .map(|task| {
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
                // Pane title budget is tight; model still lands in the bottom bar.
                agent: crate::tui::route_display::format_route_fit(task, 28),
                status: task_status_label(task),
                activity: app.task_activity(task),
                prompt: task.prompt.clone(),
                events,
                tokens: task_tokens(task),
                cost: cost::format_cost_label(task.cost_usd, task.agent),
                model: task
                    .display_model()
                    .unwrap_or_else(|| "unknown".to_string()),
                milestone: app.get_milestone(task.id.as_str()).unwrap_or("").to_string(),
                cpu: task_cpu(app, task),
                memory: task_memory(app, task),
                workgroup: task.workgroup_id.clone().unwrap_or_default(),
                worktree_branch: task.worktree_branch.clone().unwrap_or_default(),
                created: task.created_at.format("%m-%d %H:%M").to_string(),
                elapsed,
                scroll_offset: app.pane_scroll_offset(task.id.as_str()),
                total_events,
            }
        })
        .collect();
    multipane::render_multipane(frame, &panes, app, app.active_pane);
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
        "ID", "Route", "Status", "Progress", "CPU", "Mem", "Created", "Duration", "Tokens", "Cost", "Model", "Prompt",
    ])
    .style(Style::default().add_modifier(Modifier::BOLD));
    let nodes = tree_data::build_task_tree_with_state(
        &app.tasks,
        &app.wg_creators,
        &app.collapsed_projects,
    );
    let rows = nodes.iter().map(|node| board_row(app, node));
    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            // cli/provider/model — longer than the old opaque agent column.
            Constraint::Length(28),
            Constraint::Length(8),
            Constraint::Length(24),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(11),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(18),
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
    if !nodes.is_empty() {
        state.select(Some(app.tree_selected.min(nodes.len() - 1)));
    }
    frame.render_stateful_widget(table, chunks[1], &mut state);

    if app.search_mode {
        frame.render_widget(
            Paragraph::new(format!(
                "Find: {}  (Enter select, Esc cancel, n/N next/previous)",
                app.search_query
            )),
            chunks[2],
        );
    } else {
        render_status_bar(frame, chunks[2], app, StatusBarMode::Board);
    }
}

fn board_row(
    app: &App,
    node: &TreeNode,
) -> Row<'static> {
    if node.is_group_header {
        return Row::new(group_header_cells(&node.prefix))
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    }
    task_row(app, &node.task)
}

const TABLE_COLUMN_WIDTHS: [usize; 12] = [10, 28, 8, 24, 7, 7, 11, 10, 8, 8, 18, 20];

fn group_header_cells(prefix: &str) -> Vec<Cell<'static>> {
    let text = fit_group_header(prefix, TABLE_COLUMN_WIDTHS.iter().sum());
    let mut chars = text.chars();
    TABLE_COLUMN_WIDTHS
        .iter()
        .map(|width| Cell::from(chars.by_ref().take(*width).collect::<String>()))
        .collect()
}

fn fit_group_header(prefix: &str, width: usize) -> String {
    let value = prefix.trim_end();
    let Some((label, count)) = value.rsplit_once(" (") else {
        return truncate(value, width);
    };
    let suffix = format!(" ({count}");
    let label_width = width.saturating_sub(suffix.chars().count());
    let visible_label: String = label.chars().take(label_width).collect();
    format!("{visible_label}{suffix}")
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
    use super::{fit_group_header, status_to_color, TABLE_COLUMN_WIDTHS};
    use ratatui::prelude::Color;
    use crate::types::TaskStatus;

    #[test]
    fn status_to_color_maps_terminal_states() {
        assert_eq!(status_to_color(TaskStatus::Done), Color::Green);
        assert_eq!(status_to_color(TaskStatus::Merged), Color::Green);
        assert_eq!(status_to_color(TaskStatus::Failed), Color::Red);
        assert_eq!(status_to_color(TaskStatus::Running), Color::Yellow);
    }

    #[test]
    fn group_header_parts_keep_label_and_count_across_the_row() {
        let prefix = "▾ agentswap-cli (2/3) ";
        let shown = fit_group_header(prefix, TABLE_COLUMN_WIDTHS.iter().sum());

        assert_eq!(shown, "▾ agentswap-cli (2/3)");
        assert!(shown.len() > 10);
        assert!(shown.ends_with("(2/3)"));
    }

    #[test]
    fn long_group_header_preserves_count() {
        let prefix = format!("▾ {} (14/14) ", "project-".repeat(30));
        let shown = fit_group_header(&prefix, 20);

        assert!(shown.ends_with("(14/14)"));
    }
}
