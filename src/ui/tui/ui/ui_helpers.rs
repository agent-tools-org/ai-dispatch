// Helper and formatting functions for the TUI board and detail views.
// Extracted from ui.rs to keep individual files under 300 lines.

use crate::ui::format::truncate;
use ratatui::prelude::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row};

use crate::cost;
use crate::types::{Task, TaskStatus};
use crate::ui::tui::app::{App, DetailTab};
use crate::ui::tui::route_display::format_route_fit;

pub fn task_status_label(task: &Task) -> String {
    if task.status == TaskStatus::AwaitingInput {
        return "WAITING".to_string();
    }
    let label = task.status.label();
    task.outcome()
        .verification_tag()
        .map(|tag| format!("{label} [{tag}]"))
        .unwrap_or_else(|| label.to_string())
}

pub fn task_row(app: &App, task: &Task) -> Row<'static> {
    let label = task_status_label(task);
    let status = match task.status {
        TaskStatus::Running => format!("▶ {label}"),
        TaskStatus::AwaitingInput => label,
        TaskStatus::Stalled => format!("! {label}"),
        TaskStatus::Done | TaskStatus::Merged => format!("✓ {label}"),
        TaskStatus::Failed => format!("✗ {label}"),
        TaskStatus::Stopped => format!("✗ {label}"),
        _ => label,
    };
    // Route column budget matches the table Constraint::Length(28).
    let route = format_route_fit(task, 28);
    let model = task
        .display_model()
        .unwrap_or_else(|| "unknown".to_string());
    Row::new(vec![
        Cell::from(task.id.as_str().to_string()),
        Cell::from(route),
        Cell::from(status),
        Cell::from(task_progress(app, task)),
        Cell::from(task_cpu(app, task)),
        Cell::from(task_memory(app, task)),
        Cell::from(task.created_at.format("%m-%d %H:%M").to_string()),
        Cell::from(task_duration(task)),
        Cell::from(task_tokens(task)),
        Cell::from(cost::format_cost_label(task.cost_usd, task.agent)),
        Cell::from(truncate(&model, 18)),
        Cell::from(truncate(&task.prompt, 60)),
    ])
    .style(status_style(task.status))
}

pub fn task_header(task: &Task, events: &[crate::types::TaskEvent]) -> Paragraph<'static> {
    let line1 = task_identity_line(task);
    let line2 = Line::from(vec![
        Span::styled("Duration: ", Style::default().fg(Color::Indexed(243))),
        Span::raw(task_duration(task)),
        Span::styled("  Tokens: ", Style::default().fg(Color::Indexed(243))),
        Span::raw(task_tokens(task)),
        Span::styled("  Cost: ", Style::default().fg(Color::Indexed(243))),
        Span::raw(cost::format_cost_label(task.cost_usd, task.agent)),
    ]);
    let scope = task_scope_line(task);
    let mut lines = vec![line1, line2];
    if !scope.is_empty() {
        lines.push(Line::from(Span::styled(
            scope,
            Style::default().fg(Color::Indexed(243)),
        )));
    }
    if task.status == TaskStatus::AwaitingInput {
        let latest = events.last();
        lines.push(Line::from(Span::styled(
            crate::ui::tui::agent_state::activity_label(
                task.status,
                task.agent,
                task.id.as_str(),
                latest,
            ),
            Style::default().fg(Color::Magenta),
        )));
    }
    if task.status == TaskStatus::Failed
        && let Some(reason) = last_error_detail(events)
    {
        lines.push(Line::from(Span::styled(
            format!("Reason: {}", truncate(&reason, 120)),
            Style::default().fg(Color::Red),
        )));
    }
    Paragraph::new(lines)
}

pub fn tab_bar(active: DetailTab) -> Paragraph<'static> {
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
                Style::default().fg(Color::Indexed(245))
            };
            [Span::styled(format!(" {label} "), style), Span::raw(" ")]
        })
        .collect();
    Paragraph::new(Line::from(spans))
}

pub fn detail_content_block(title: &'static str) -> Block<'static> {
    Block::default().title(title).borders(Borders::TOP)
}
pub fn task_scope_line(task: &Task) -> String {
    match (task.workgroup_id.as_deref(), task.worktree_path.as_deref()) {
        (Some(group), Some(worktree)) => {
            format!("Group: {group}  Worktree: {}", truncate(worktree, 80))
        }
        (Some(group), None) => format!("Group: {group}"),
        (None, Some(worktree)) => format!("Worktree: {}", truncate(worktree, 96)),
        (None, None) => String::new(),
    }
}

pub fn prompt_text(task: &Task) -> String {
    if let Some(resolved) = &task.resolved_prompt {
        format!(
            "--- Original Prompt ---\n{}\n\n--- Resolved Prompt ---\n{}",
            task.prompt, resolved
        )
    } else {
        task.prompt.clone()
    }
}

#[cfg(test)]
pub fn read_task_output_for_tui(task: &Task) -> String {
    crate::task::view::read_output(task)
}

pub fn detail_scroll_offset(detail_scroll: usize) -> u16 {
    detail_scroll.min(u16::MAX as usize) as u16
}

pub fn task_duration(task: &Task) -> String {
    task.duration_ms
        .map(crate::ui::format::format_duration_ms)
        .unwrap_or_else(|| "-".to_string())
}

pub fn task_tokens(task: &Task) -> String {
    task.tokens
        .map(crate::ui::format::format_tokens)
        .unwrap_or_else(|| "-".to_string())
}

pub fn task_cpu(app: &App, task: &Task) -> String {
    app.get_metrics(task.id.as_str())
        .map(|metrics| format!("{:.1}%", metrics.cpu_percent))
        .unwrap_or_else(|| "—".to_string())
}

pub fn task_memory(app: &App, task: &Task) -> String {
    app.get_metrics(task.id.as_str())
        .map(|metrics| format!("{:.0}M", metrics.memory_mb))
        .unwrap_or_else(|| "—".to_string())
}

pub fn task_progress(app: &App, task: &Task) -> String {
    if matches!(task.status, TaskStatus::Running | TaskStatus::AwaitingInput) {
        return truncate(&app.task_activity(task), 30);
    }
    // For failed tasks, show last error reason instead of milestone
    if task.status == TaskStatus::Failed
        && let Some(reason) = app.get_failure_reason(task.id.as_str())
    {
        return truncate(&reason, 30);
    }
    let milestone_or_dash = || {
        app.get_milestone(task.id.as_str())
            .map(|milestone| truncate(milestone, 30))
            .unwrap_or_else(|| "—".to_string())
    };
    match task.status {
        TaskStatus::Running | TaskStatus::Stalled | TaskStatus::Done | TaskStatus::Merged => {
            milestone_or_dash()
        }
        _ => "—".to_string(),
    }
}

/// Row style for the default board view.
///
/// Success is green. It rendered as grey 245 here while the identically-named
/// `dashboard::status_style` used green for the same status, so which colour a
/// completed task got depended on which view you were in — and the default view,
/// the one `aid watch --tui` opens, was the grey one.
pub fn status_style(status: TaskStatus) -> Style {
    match status {
        TaskStatus::Done | TaskStatus::Merged => Style::default().fg(Color::Green),
        TaskStatus::Running => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        TaskStatus::AwaitingInput => Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
        TaskStatus::Stalled => Style::default()
            .fg(Color::LightRed)
            .add_modifier(Modifier::BOLD),
        TaskStatus::Failed => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        TaskStatus::Stopped => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        TaskStatus::Pending => Style::default().fg(Color::Indexed(250)),
        TaskStatus::Waiting => Style::default().fg(Color::Indexed(240)),
        TaskStatus::Skipped => Style::default().fg(Color::Blue),
    }
}

/// Pick the most informative Error event to surface as the failure reason.
///
/// The last error is often a cascading verification failure rather than the
/// trigger. Prefer a recognized trigger, then fall back to the first error.
fn last_error_detail(events: &[crate::types::TaskEvent]) -> Option<String> {
    let mut first_error = None;
    for event in events {
        if event.event_kind != crate::types::EventKind::Error {
            continue;
        }
        if first_error.is_none() {
            first_error = Some(event.detail.clone());
        }
        if is_trigger_error(&event.detail) {
            return Some(event.detail.clone());
        }
    }
    first_error
}

fn is_trigger_error(detail: &str) -> bool {
    const TRIGGERS: &[&str] = &[
        "apply_patch",
        "command failed",
        "rate limit",
        "killed:",
        "task killed",
        "exceeded ceiling",
    ];
    let detail = detail.to_ascii_lowercase();
    TRIGGERS.iter().any(|trigger| detail.contains(trigger))
}

#[cfg(test)]
#[path = "ui_helpers_tests.rs"]
mod tests;

fn task_identity_line(task: &Task) -> Line<'static> {
    let status_color = match task.status {
        TaskStatus::Done | TaskStatus::Merged => Color::Green,
        TaskStatus::Running => Color::Yellow,
        TaskStatus::AwaitingInput => Color::Magenta,
        TaskStatus::Stalled => Color::LightRed,
        TaskStatus::Failed => Color::Red,
        TaskStatus::Stopped => Color::Yellow,
        _ => Color::Indexed(250),
    };
    // Detail has room for the full triple; attribution rides on the model segment.
    // No separate Model line — Route already carries model + grade.
    let route = task.display_route();
    let line1 = Line::from(vec![
        Span::styled(
            task.id.as_str().to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(route, Style::default().fg(Color::Indexed(250))),
        Span::raw("  "),
        Span::styled(
            task_status_label(task),
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    line1
}
