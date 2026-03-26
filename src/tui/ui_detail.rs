// Task detail panel: full-screen task view with tabs and scrollable content.
// Exports render_detail and render_detail_content for parent `ui`.
// Depends on ratatui, `crate::tui::app`, and sibling `ui_helpers`.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use crate::tui::app::{App, DetailTab};
use super::ui_helpers::{
    detail_content_block, detail_scroll_offset, prompt_text, read_task_output_for_tui, tab_bar,
    task_header,
};
use crate::types::{EventKind, Task, TaskEvent};

pub(super) fn render_detail(frame: &mut ratatui::Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(1)])
        .split(frame.area());
    let detail_block = Block::default().title("Task").borders(Borders::ALL);
    let inner = detail_block.inner(chunks[0]);
    frame.render_widget(detail_block, chunks[0]);

    if let Some(task) = app.selected_task() {
        let events = app.selected_events();
        let detail_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4),
                Constraint::Length(1),
                Constraint::Min(1),
            ])
            .split(inner);
        frame.render_widget(task_header(task, &events), detail_chunks[0]);
        frame.render_widget(tab_bar(app.detail_tab), detail_chunks[1]);
        render_detail_content(frame, detail_chunks[2], app, task, &events);
    } else {
        frame.render_widget(
            Paragraph::new(app.empty_message()),
            inner,
        );
    }

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "e=events p=prompt o=output Tab=next Esc=back q=quit",
            Style::default().fg(Color::Indexed(243)),
        ))),
        chunks[1],
    );
}

pub(super) fn render_detail_content(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    task: &Task,
    events: &[TaskEvent],
) {
    match app.detail_tab {
        DetailTab::Events => {
            let items: Vec<ListItem<'_>> = events
                .iter()
                .map(|event| {
                    let kind_color = event_kind_color(event.event_kind);
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            event.timestamp.format("%H:%M:%S").to_string(),
                            Style::default().fg(Color::Indexed(243)),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            format!("[{}]", event.event_kind.as_str()),
                            Style::default().fg(kind_color),
                        ),
                        Span::raw(" "),
                        Span::raw(event.detail.clone()),
                    ]))
                })
                .collect();
            frame.render_widget(
                List::new(items).block(detail_content_block("Events")),
                area,
            );
        }
        DetailTab::Prompt => {
            frame.render_widget(
                Paragraph::new(prompt_text(task))
                    .wrap(Wrap { trim: false })
                    .scroll((detail_scroll_offset(app.detail_scroll), 0))
                    .block(detail_content_block("Prompt")),
                area,
            );
        }
        DetailTab::Output => {
            frame.render_widget(
                Paragraph::new(read_task_output_for_tui(task))
                    .wrap(Wrap { trim: false })
                    .scroll((detail_scroll_offset(app.detail_scroll), 0))
                    .block(detail_content_block("Output")),
                area,
            );
        }
    }
}

fn event_kind_color(kind: EventKind) -> Color {
    match kind {
        EventKind::Milestone => Color::Green,
        EventKind::Error => Color::Red,
        EventKind::Completion => Color::Cyan,
        EventKind::ToolCall => Color::Yellow,
        EventKind::Build | EventKind::Test => Color::Blue,
        _ => Color::Indexed(245),
    }
}

#[cfg(test)]
mod tests {
    use super::event_kind_color;
    use crate::types::EventKind;
    use ratatui::prelude::Color;

    #[test]
    fn event_kind_color_matches_event_list_styling() {
        assert_eq!(event_kind_color(EventKind::Milestone), Color::Green);
        assert_eq!(event_kind_color(EventKind::Error), Color::Red);
        assert_eq!(event_kind_color(EventKind::ToolCall), Color::Yellow);
    }
}
