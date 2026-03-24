// ratatui rendering for the aid dashboard board and detail screens.
// Draws table/list widgets from App state with simple status coloring.

#[path = "ui_helpers.rs"]
mod ui_helpers;
use ui_helpers::*;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::{Alignment, Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, List, ListItem, Paragraph, Row, Table, TableState, Wrap,
};

use super::app::{App, DetailTab};
use super::charts;
use super::dashboard;
use super::multipane;
use super::tree_data;
use crate::cost;
use crate::types::TaskStatus;

pub fn render(frame: &mut ratatui::Frame<'_>, app: &App) {
    if app.tree_mode {
        render_tree_view(frame, app);
    } else if app.multipane_mode {
        render_multipane_view(frame, app);
    } else if app.detail_mode {
        render_detail(frame, app);
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
    let running = app.tasks.iter().filter(|task| matches!(task.status, TaskStatus::Running | TaskStatus::AwaitingInput)).count();
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

fn render_tree_view(frame: &mut ratatui::Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let done = app.tasks.iter().filter(|t| t.status.is_terminal()).count();
    let running = app.tasks.iter().filter(|t| matches!(t.status, TaskStatus::Running)).count();
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("aid tree ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(format!("[{}]", app.scope_label()), Style::default().fg(Color::Indexed(250))),
            Span::raw("  "),
            Span::styled(format!("{done}✓ "), Style::default().fg(Color::Green)),
            Span::styled(format!("{running}▶"), Style::default().fg(Color::Yellow)),
        ]))
        .alignment(Alignment::Center),
        chunks[0],
    );

    let nodes = tree_data::build_task_tree_with_creators(&app.tasks, &app.wg_creators);
    // We can't mutate app here (render takes &App), so tree_node_count
    // is updated in tick(). Use nodes.len() for bounds checking.
    if nodes.is_empty() {
        frame.render_widget(Paragraph::new(app.empty_message()), chunks[1]);
    } else {
        let items: Vec<ListItem> = nodes
            .iter()
            .enumerate()
            .map(|(i, node)| {
                let task = &node.task;
                let status_color = status_to_color(task.status);
                let is_selected = i == app.tree_selected;

                if node.is_group_header {
                    // Workgroup header line
                    let running_in_group = app.tasks.iter()
                        .filter(|t| t.workgroup_id.as_deref() == task.workgroup_id.as_deref()
                            && matches!(t.status, TaskStatus::Running))
                        .count();
                    let total_in_group = app.tasks.iter()
                        .filter(|t| t.workgroup_id.as_deref() == task.workgroup_id.as_deref())
                        .count();
                    let done_in_group = app.tasks.iter()
                        .filter(|t| t.workgroup_id.as_deref() == task.workgroup_id.as_deref() && t.status.is_terminal())
                        .count();
                    let mut spans = vec![
                        Span::styled(&node.prefix, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        Span::styled(
                            format!(" ({done_in_group}/{total_in_group})"),
                            Style::default().fg(Color::Indexed(243)),
                        ),
                    ];
                    if running_in_group > 0 {
                        spans.push(Span::styled(
                            format!(" {running_in_group}▶"),
                            Style::default().fg(Color::Yellow),
                        ));
                    }
                    let item = ListItem::new(Line::from(spans));
                    if is_selected {
                        item.style(Style::default().bg(Color::Indexed(237)).add_modifier(Modifier::BOLD))
                    } else {
                        item
                    }
                } else {
                    // Task line
                    let duration = tree_duration(task);
                    let milestone = app.get_milestone(task.id.as_str()).map(|m| truncate(m, 25));
                    let cost_str = cost::format_cost_label(task.cost_usd, task.agent);
                    let prompt_width = if milestone.is_some() { 25 } else { 40 };
                    let prompt_preview = truncate(&task.prompt, prompt_width);

                    let is_done = matches!(task.status, TaskStatus::Done | TaskStatus::Merged);
                    let id_color = if is_done { Color::Green } else { Color::White };
                    let dim = Color::Indexed(if is_done { 243 } else { 248 });

                    let mut spans = vec![
                        Span::styled(node.prefix.clone(), Style::default().fg(Color::Indexed(240))),
                        Span::styled(task.id.as_str(), Style::default().fg(id_color).add_modifier(Modifier::BOLD)),
                        Span::raw(" "),
                        Span::styled(task.agent_display_name().to_string(), Style::default().fg(if is_done { Color::Green } else { Color::Cyan })),
                        Span::raw(" "),
                        Span::styled(task.status.label().to_string(), Style::default().fg(status_color)),
                        Span::raw(" "),
                        Span::styled(duration, Style::default().fg(dim)),
                    ];
                    if cost_str != "—" && cost_str != "-" {
                        spans.push(Span::raw(" "));
                        spans.push(Span::styled(cost_str, Style::default().fg(Color::Indexed(243))));
                    }
                    if let Some(ms) = milestone {
                        spans.push(Span::raw(" "));
                        spans.push(Span::styled(ms, Style::default().fg(Color::Green)));
                    } else {
                        spans.push(Span::raw("  "));
                        spans.push(Span::styled(prompt_preview, Style::default().fg(Color::Indexed(245))));
                    }

                    let item = ListItem::new(Line::from(spans));
                    if is_selected {
                        item.style(Style::default().bg(Color::Indexed(237)).add_modifier(Modifier::BOLD))
                    } else {
                        item
                    }
                }
            })
            .collect();
        frame.render_widget(
            List::new(items).block(Block::default().title("Task Tree").borders(Borders::ALL)),
            chunks[1],
        );
    }

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" j/k", Style::default().fg(Color::Yellow)),
            Span::raw(":nav "),
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
        chunks[2],
    );
}

fn tree_duration(task: &crate::types::Task) -> String {
    if let Some(ms) = task.duration_ms {
        let secs = ms / 1000;
        if secs < 60 { format!("{secs}s") }
        else if secs < 3600 { format!("{}m{:02}s", secs / 60, secs % 60) }
        else { format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60) }
    } else if matches!(task.status, TaskStatus::Running | TaskStatus::AwaitingInput) {
        let elapsed = (chrono::Local::now() - task.created_at).num_seconds();
        if elapsed < 60 { format!("{elapsed}s") }
        else if elapsed < 3600 { format!("{}m{:02}s", elapsed / 60, elapsed % 60) }
        else { format!("{}h{:02}m", elapsed / 3600, (elapsed % 3600) / 60) }
    } else {
        "-".to_string()
    }
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
        TaskStatus::Skipped => Color::Blue,
    }
}

fn render_detail(frame: &mut ratatui::Frame<'_>, app: &App) {
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

fn render_detail_content(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    task: &crate::types::Task,
    events: &[crate::types::TaskEvent],
) {
    match app.detail_tab {
        DetailTab::Events => {
            let items: Vec<ListItem<'_>> = events
                .iter()
                .map(|event| {
                    let kind_color = match event.event_kind {
                        crate::types::EventKind::Milestone => Color::Green,
                        crate::types::EventKind::Error => Color::Red,
                        crate::types::EventKind::Completion => Color::Cyan,
                        crate::types::EventKind::ToolCall => Color::Yellow,
                        crate::types::EventKind::Build | crate::types::EventKind::Test => Color::Blue,
                        _ => Color::Indexed(245),
                    };
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
