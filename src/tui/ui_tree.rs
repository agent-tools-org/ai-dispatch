// Task tree view: hierarchical list of tasks and workgroups with status lines.
// Exports render_tree_view; uses tree_duration for per-row duration labels.
// Depends on ratatui, app, tree_data, cost, types, and parent status_to_color.

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::prelude::{Alignment, Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::tui::app::App;
use super::status_to_color;
use super::ui_helpers::truncate;
use crate::cost;
use crate::tui::tree_data;
use crate::types::{Task, TaskStatus};

pub(super) fn render_tree_view(frame: &mut ratatui::Frame<'_>, app: &App) {
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

fn tree_duration(task: &Task) -> String {
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

#[cfg(test)]
mod tests {
    use super::tree_duration;
    use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
    use chrono::Local;

    fn base_task() -> Task {
        Task {
            id: TaskId("t-tree".to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "p".to_string(),
            resolved_prompt: None,
            category: None,
            status: TaskStatus::Done,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            start_sha: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            pending_reason: None,
            read_only: false,
            budget: false,
        }
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
}
