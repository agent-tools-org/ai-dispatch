// Task and group row construction for the tree view.
// Uses the parent tree duration and group label formatting helpers.

use super::status_to_color;
use super::tree::{fit_group_header, tree_duration};
use super::ui_helpers::task_status_label;
use crate::cost;
use crate::types::{Task, TaskStatus};
use crate::ui::format::truncate;
use crate::ui::tui::app::App;
use crate::ui::tui::route_display::format_route_fit;
use ratatui::prelude::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::ListItem;

pub(super) fn tree_item<'a>(
    app: &'a App,
    i: usize,
    node: &crate::ui::tui::tree_data::TreeNode,
    width: usize,
) -> ListItem<'a> {
    let task = &app.tasks[node.task_index];
    let is_selected = i == app.tree_selected;

    if node.is_group_header {
        let header = fit_group_header(&node.prefix, width);
        let item = ListItem::new(Span::styled(
            header,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        if is_selected {
            item.style(
                Style::default()
                    .bg(Color::Indexed(237))
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            item
        }
    } else {
        let spans = task_spans(app, task, node);
        let item = ListItem::new(Line::from(spans));
        if is_selected {
            item.style(
                Style::default()
                    .bg(Color::Indexed(237))
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            item
        }
    }
}

fn task_spans<'a>(
    app: &App,
    task: &'a Task,
    node: &crate::ui::tui::tree_data::TreeNode,
) -> Vec<Span<'a>> {
    let status_color = status_to_color(task.status);
    // Task line
    let duration = tree_duration(task);
    let milestone = app.get_milestone(task.id.as_str()).map(|m| truncate(m, 25));
    let cost_str = cost::format_cost_label(task.cost_usd, task.agent);
    let prompt_width = if milestone.is_some() { 25 } else { 40 };
    let prompt_preview = truncate(&task.prompt, prompt_width);

    let is_done = matches!(task.status, TaskStatus::Done | TaskStatus::Merged);
    let id_color = if is_done { Color::Green } else { Color::White };
    // Success must read as success. Dimming a finished row harder
    // than a running one made the whole line grey, so a completed
    // task looked spent rather than good — the green id and route
    // were not enough to carry it.
    let dim = Color::Indexed(248);

    let mut spans = task_identity_spans(task, node, duration, is_done, id_color, dim, status_color);
    append_task_activity(&mut spans, app, task);
    if cost_str != "—" && cost_str != "-" {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            cost_str,
            Style::default().fg(if is_done {
                Color::Green
            } else {
                Color::Indexed(243)
            }),
        ));
    }
    if let Some(ms) = milestone {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(ms, Style::default().fg(Color::Green)));
    } else {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            prompt_preview,
            Style::default().fg(Color::Indexed(245)),
        ));
    }

    spans
}

fn append_task_activity(spans: &mut Vec<Span<'_>>, app: &App, task: &Task) {
    if matches!(task.status, TaskStatus::Running | TaskStatus::AwaitingInput) {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            truncate(&app.task_activity(task), 42),
            Style::default().fg(if task.status == TaskStatus::AwaitingInput {
                Color::Magenta
            } else {
                Color::Indexed(245)
            }),
        ));
    }
}

fn task_identity_spans<'a>(
    task: &'a Task,
    node: &crate::ui::tui::tree_data::TreeNode,
    duration: String,
    is_done: bool,
    id_color: Color,
    dim: Color,
    status_color: Color,
) -> Vec<Span<'a>> {
    // Tree rows are dense: 22 chars keeps route visible without
    // crowding status/duration. Truncation drops provider first.
    let route = format_route_fit(task, 22);
    let spans = vec![
        Span::styled(
            node.prefix.clone(),
            Style::default().fg(Color::Indexed(240)),
        ),
        Span::styled(
            task.id.as_str(),
            Style::default().fg(id_color).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            route,
            Style::default().fg(if is_done { Color::Green } else { Color::Cyan }),
        ),
        Span::raw(" "),
        Span::styled(task_status_label(task), Style::default().fg(status_color)),
        Span::raw(" "),
        Span::styled(duration, Style::default().fg(dim)),
    ];
    spans
}
