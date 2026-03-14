// ratatui rendering for the aid dashboard board and detail screens.
// Draws table/list widgets from App state with simple status coloring.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::{Alignment, Color, Modifier, Style};
use ratatui::text::{Line, Span};
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
    Wrap,
};

use super::app::{App, DetailTab};
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
        Paragraph::new("e=events p=prompt o=output Tab=next Esc=back q=quit"),
        chunks[1],
    );
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

fn render_detail_content(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    app: &App,
    task: &Task,
    events: &[crate::types::TaskEvent],
) {
    match app.detail_tab {
        DetailTab::Events => {
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

fn task_header(task: &Task, events: &[crate::types::TaskEvent]) -> Paragraph<'static> {
    let awaiting = if task.status == TaskStatus::AwaitingInput {
        pending_prompt(events)
            .map(|prompt| format!("Awaiting: {}", truncate(prompt, 120)))
            .unwrap_or_default()
    } else {
        String::new()
    };
    Paragraph::new(
        [
            format!("{}  {}  {}", task.id, task.agent, task.status.label()),
            format!(
                "Duration: {}  Tokens: {}  Cost: {}  Model: {}",
                task_duration(task),
                task_tokens(task),
                cost::format_cost(task.cost_usd),
                task.model.as_deref().unwrap_or("-"),
            ),
            task_scope_line(task),
            awaiting,
        ]
        .join("\n"),
    )
}

fn tab_bar(active: DetailTab) -> Paragraph<'static> {
    let tabs = [
        ("Events", DetailTab::Events),
        ("Prompt", DetailTab::Prompt),
        ("Output", DetailTab::Output),
    ];
    let spans: Vec<Span<'static>> = tabs
        .iter()
        .flat_map(|(label, tab)| {
            let style = if *tab == active {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            [Span::styled(format!(" {label} "), style), Span::raw(" ")]
        })
        .collect();
    Paragraph::new(Line::from(spans))
}

fn detail_content_block(title: &'static str) -> Block<'static> {
    Block::default().title(title).borders(Borders::TOP)
}

fn task_scope_line(task: &Task) -> String {
    match (task.workgroup_id.as_deref(), task.worktree_path.as_deref()) {
        (Some(group), Some(worktree)) => {
            format!("Group: {group}  Worktree: {}", truncate(worktree, 80))
        }
        (Some(group), None) => format!("Group: {group}"),
        (None, Some(worktree)) => format!("Worktree: {}", truncate(worktree, 96)),
        (None, None) => String::new(),
    }
}

fn prompt_text(task: &Task) -> String {
    if let Some(resolved) = &task.resolved_prompt {
        format!(
            "--- Original Prompt ---\n{}\n\n--- Resolved Prompt ---\n{}",
            task.prompt, resolved
        )
    } else {
        task.prompt.clone()
    }
}

fn read_task_output_for_tui(task: &Task) -> String {
    if let Some(path) = task.output_path.as_deref()
        && let Ok(content) = std::fs::read_to_string(path)
    {
        return content;
    }
    if let Some(path) = task.log_path.as_deref()
        && let Ok(content) = std::fs::read_to_string(path)
    {
        let output = content
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter_map(|value| {
                value
                    .get("content")
                    .and_then(|content| content.as_str())
                    .map(String::from)
            })
            .collect::<Vec<_>>()
            .join("");
        if !output.is_empty() {
            return output;
        }
    }
    "No output available".to_string()
}

fn detail_scroll_offset(detail_scroll: usize) -> u16 {
    detail_scroll.min(u16::MAX as usize) as u16
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AgentKind, TaskId};
    use chrono::Local;
    use tempfile::NamedTempFile;

    fn make_task() -> Task {
        Task {
            id: TaskId("t-ui".to_string()),
            agent: AgentKind::Codex,
            prompt: "prompt".to_string(),
            resolved_prompt: None,
            status: TaskStatus::Done,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            read_only: false,
            budget: false,
        }
    }

    #[test]
    fn detail_output_prefers_output_file() {
        let output_file = NamedTempFile::new().unwrap();
        std::fs::write(output_file.path(), "stdout\n").unwrap();
        let mut task = make_task();
        task.output_path = Some(output_file.path().display().to_string());

        assert_eq!(read_task_output_for_tui(&task), "stdout\n");
    }

    #[test]
    fn detail_output_parses_log_jsonl_content() {
        let log_file = NamedTempFile::new().unwrap();
        std::fs::write(
            log_file.path(),
            "{\"content\":\"hello\\n\"}\n{\"content\":\"world\"}\n",
        )
        .unwrap();
        let mut task = make_task();
        task.log_path = Some(log_file.path().display().to_string());

        assert_eq!(read_task_output_for_tui(&task), "hello\nworld");
    }
}
