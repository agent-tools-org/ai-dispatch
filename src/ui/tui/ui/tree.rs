// Task tree view: hierarchical list of tasks and workgroups with status lines.
// Exports render_tree_view; uses tree_duration for per-row duration labels.
// Depends on ratatui, app, tree_data, cost, types, and parent status_to_color.

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::prelude::{Alignment, Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use super::tree_rows::tree_item;
use crate::types::{Task, TaskStatus};
use crate::ui::format::truncate;
use crate::ui::tui::app::App;

pub(super) fn render_tree_view(frame: &mut ratatui::Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    render_tree_header(frame, app, chunks[0]);

    let nodes = &app.nodes;
    let height = chunks[1].height.saturating_sub(2) as usize;
    let start = app.tree_selected.saturating_sub(height.saturating_sub(1));
    if nodes.is_empty() {
        frame.render_widget(Paragraph::new(app.empty_message()), chunks[1]);
    } else {
        let items: Vec<ListItem> = nodes
            .iter()
            .enumerate()
            .skip(start)
            .take(height)
            .map(|(i, node)| tree_item(app, i, node, chunks[1].width as usize))
            .collect();
        frame.render_widget(
            List::new(items).block(Block::default().title("Task Tree").borders(Borders::ALL)),
            chunks[1],
        );
    }

    render_tree_footer(frame, chunks[2]);
}

pub(super) fn fit_group_header(prefix: &str, width: usize) -> String {
    let value = prefix.trim_end();
    let Some((label, count)) = value.rsplit_once(" (") else {
        return truncate(value, width);
    };
    let suffix = format!(" ({count}");
    let label_width = width.saturating_sub(suffix.chars().count());
    let visible_label: String = label.chars().take(label_width).collect();
    format!("{visible_label}{suffix}")
}

pub(super) fn tree_duration(task: &Task) -> String {
    if let Some(ms) = task.duration_ms {
        let secs = ms / 1000;
        if secs < 60 {
            format!("{secs}s")
        } else if secs < 3600 {
            format!("{}m{:02}s", secs / 60, secs % 60)
        } else {
            format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60)
        }
    } else if matches!(
        task.status,
        TaskStatus::Running | TaskStatus::AwaitingInput | TaskStatus::Stalled
    ) {
        let elapsed = (chrono::Local::now() - task.created_at).num_seconds();
        if elapsed < 60 {
            format!("{elapsed}s")
        } else if elapsed < 3600 {
            format!("{}m{:02}s", elapsed / 60, elapsed % 60)
        } else {
            format!("{}h{:02}m", elapsed / 3600, (elapsed % 3600) / 60)
        }
    } else {
        "-".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{fit_group_header, tree_duration};
    use crate::ui::tui::route_display::format_route_fit;
    use crate::types::{Task, TaskId, TaskStatus};


    fn base_task() -> Task {
        Task {
            prompt: "p".to_string(),
            project_id: crate::config::project::current_project_id(),
            ..crate::types::Task::test_fixture(TaskId("t-tree".to_string()), TaskStatus::Done)
        }
    }

    #[test]
    fn tree_route_fit_uses_provider_not_just_cli() {
        use crate::types::AttributionSource;
        let mut t = base_task();
        t.requested_model = Some("gpt-5.6".to_string());
        t.observed_model = Some("gpt-5.6".to_string());
        t.attribution_source = Some(AttributionSource::ConfirmedBySuccess);
        let shown = format_route_fit(&t, 22);
        assert!(shown.starts_with("codex/"), "{shown}");
        assert!(shown.contains("openai") || shown.contains("..."), "{shown}");
        // inferred attribution is part of the model segment when width allows
        let wide = format_route_fit(&t, 80);
        assert!(wide.contains("(inferred)"), "{wide}");
    }

    #[test]
    fn tree_duration_formats_completed_seconds() {
        let mut t = base_task();
        t.duration_ms = Some(45_000);
        assert_eq!(tree_duration(&t), "45s");
    }

    #[test]
    fn tree_duration_formats_completed_minutes() {
        let mut t = base_task();
        t.duration_ms = Some(125_000);
        assert_eq!(tree_duration(&t), "2m05s");
    }

    #[test]
    fn tree_duration_non_running_without_duration_is_dash() {
        let mut t = base_task();
        t.status = TaskStatus::Done;
        t.duration_ms = None;
        assert_eq!(tree_duration(&t), "-");
    }

    #[test]
    fn group_header_fit_preserves_count_after_long_project_label() {
        let shown = fit_group_header("▸ project-with-a-long-id (1/42)", 12);
        assert!(shown.contains("(1/42)"), "{shown}");
        assert!(shown.chars().count() <= 12, "{shown}");
    }
}

fn render_tree_header(frame: &mut ratatui::Frame<'_>, app: &App, area: ratatui::layout::Rect) {
    let done = app.tasks.iter().filter(|t| t.status.is_terminal()).count();
    let running = app
        .tasks
        .iter()
        .filter(|t| matches!(t.status, TaskStatus::Running))
        .count();
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "aid tree ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("[{}]", app.scope_label()),
                Style::default().fg(Color::Indexed(250)),
            ),
            Span::raw("  "),
            Span::styled(format!("{done}✓ "), Style::default().fg(Color::Green)),
            Span::styled(format!("{running}▶"), Style::default().fg(Color::Yellow)),
        ]))
        .alignment(Alignment::Center),
        area,
    );
}

fn render_tree_footer(frame: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect) {
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" j/k", Style::default().fg(Color::Yellow)),
            Span::raw(":tasks "),
            Span::styled("h/l", Style::default().fg(Color::Yellow)),
            Span::raw(":groups "),
            Span::styled("Space", Style::default().fg(Color::Yellow)),
            Span::raw(":collapse "),
            Span::styled("/", Style::default().fg(Color::Yellow)),
            Span::raw(":find "),
            Span::styled("Enter", Style::default().fg(Color::Yellow)),
            Span::raw(":detail "),
            Span::styled("t", Style::default().fg(Color::Yellow)),
            Span::raw(":table "),
            Span::styled("d", Style::default().fg(Color::Yellow)),
            Span::raw(":dashboard "),
            Span::styled("m", Style::default().fg(Color::Yellow)),
            Span::raw(":multipane "),
            Span::styled("q", Style::default().fg(Color::Yellow)),
            Span::raw(":quit"),
        ])),
        area,
    );
}
