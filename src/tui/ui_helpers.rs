// Helper and formatting functions for the TUI board and detail views.
// Extracted from ui.rs to keep individual files under 300 lines.

use ratatui::prelude::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row};

use crate::cost;
use crate::tui::app::{App, DetailTab};
use crate::tui::route_display::format_route_fit;
use crate::types::{Task, TaskStatus};

pub fn task_status_label(task: &Task) -> String {
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
        Cell::from(task.workgroup_id.clone().unwrap_or_else(|| "-".to_string())),
        Cell::from(truncate(&task.prompt, 60)),
    ])
    .style(status_style(task.status))
}

pub fn task_header(task: &Task, events: &[crate::types::TaskEvent]) -> Paragraph<'static> {
    let status_color = match task.status {
        TaskStatus::Done | TaskStatus::Merged => Color::Green,
        TaskStatus::Running => Color::Yellow,
        TaskStatus::AwaitingInput => Color::Magenta,
        TaskStatus::Stalled => Color::LightRed,
        TaskStatus::Failed => Color::Red,
        TaskStatus::Stopped => Color::Red,
        _ => Color::Indexed(250),
    };
    // Detail has room for the full triple; attribution rides on the model segment.
    // No separate Model line — Route already carries model + grade.
    let route = task.display_route();
    let line1 = Line::from(vec![
        Span::styled(
            task.id.as_str().to_string(),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(route, Style::default().fg(Color::Indexed(250))),
        Span::raw("  "),
        Span::styled(task_status_label(task), Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
    ]);
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
        lines.push(Line::from(Span::styled(scope, Style::default().fg(Color::Indexed(243)))));
    }
    if task.status == TaskStatus::AwaitingInput
        && let Some(prompt) = pending_prompt(events)
    {
        lines.push(Line::from(Span::styled(
            format!("Awaiting: {}", truncate(prompt, 120)),
            Style::default().fg(Color::Magenta),
        )));
    }
    if matches!(task.status, TaskStatus::Failed | TaskStatus::Stopped)
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

pub fn read_task_output_for_tui(task: &Task) -> String {
    crate::task_view::read_output(task)
}

pub fn detail_scroll_offset(detail_scroll: usize) -> u16 {
    detail_scroll.min(u16::MAX as usize) as u16
}

pub fn task_duration(task: &Task) -> String {
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

pub fn task_tokens(task: &Task) -> String {
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
    if task.status == TaskStatus::AwaitingInput {
        return "awaiting input".to_string();
    }
    // For failed/stopped tasks, show last error reason instead of milestone
    if matches!(task.status, TaskStatus::Failed | TaskStatus::Stopped)
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
        TaskStatus::Running
        | TaskStatus::Stalled
        | TaskStatus::Done
        | TaskStatus::Merged => milestone_or_dash(),
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
        TaskStatus::Running => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        TaskStatus::AwaitingInput => Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        TaskStatus::Stalled => Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
        TaskStatus::Failed => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        TaskStatus::Stopped => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
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

pub fn pending_prompt(events: &[crate::types::TaskEvent]) -> Option<&str> {
    events.iter().rev().find_map(|event| {
        let metadata = event.metadata.as_ref()?;
        metadata
            .get("awaiting_input")
            .and_then(|value| value.as_bool())
            .filter(|value| *value)
            .map(|_| event.detail.as_str())
    })
}

pub fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        value.to_string()
    } else {
        let end = max.saturating_sub(3);
        // Find a valid UTF-8 char boundary at or before `end`
        let safe = value.floor_char_boundary(end);
        format!("{}...", &value[..safe])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AgentKind, TaskId, VerifyStatus};
    use chrono::Local;
    use tempfile::NamedTempFile;

    fn make_task() -> Task {
        Task {
            id: TaskId("t-ui".to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "prompt".to_string(),
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
            final_head_sha: None,
            final_branch: None,
            start_sha: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            requested_model: None,
            observed_model: None,
            attribution_source: None,
            cost_usd: None,
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            pending_reason: None,
            read_only: false,
            budget: false,
            audit_verdict: None,
            audit_report_path: None,
            delivery_assessment: None,
        }
    }

    #[test]
    fn task_header_shows_route_triple_not_opaque_agent() {
        use crate::types::AttributionSource;
        let mut task = make_task();
        task.requested_model = Some("gpt-5.6".to_string());
        task.observed_model = Some("gpt-5.6".to_string());
        task.attribution_source = Some(AttributionSource::Echoed);
        // task_header is a Paragraph; assert the source string it is built from.
        assert_eq!(
            task.display_route(),
            "codex/openai-chatgpt-plan/gpt-5.6"
        );
        let _ = task_header(&task, &[]);
    }

    #[test]
    fn task_header_unknown_model_is_literal_unknown() {
        let task = make_task();
        assert!(task.display_route().ends_with("/unknown"));
        assert_eq!(task.display_model(), None);
        let _ = task_header(&task, &[]);
    }

    #[test]
    fn status_label_shows_inconclusive_verification() {
        let mut task = make_task();
        task.verify = Some("cargo test".to_string());
        task.verify_status = VerifyStatus::TimedOut;

        assert_eq!(task_status_label(&task), "DONE [VTIMEOUT]");
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
    fn last_error_detail_prefers_trigger_error_over_earlier_noise() {
        use crate::types::{EventKind, TaskEvent};
        let now = Local::now();
        let events = vec![
            TaskEvent {
                task_id: TaskId("t-trigger".to_string()),
                timestamp: now,
                event_kind: EventKind::Error,
                detail: "bot.rs:41: priority_baseline_ha".to_string(),
                metadata: None,
            },
            TaskEvent {
                task_id: TaskId("t-trigger".to_string()),
                timestamp: now,
                event_kind: EventKind::Error,
                detail: "apply_patch verification failed: Failed to find expected lines".to_string(),
                metadata: None,
            },
            TaskEvent {
                task_id: TaskId("t-trigger".to_string()),
                timestamp: now,
                event_kind: EventKind::Error,
                detail: "Failed during verification: cargo check ...".to_string(),
                metadata: None,
            },
        ];

        let reason = last_error_detail(&events).expect("expected a Reason");
        assert!(reason.contains("apply_patch"));
    }

    #[test]
    fn last_error_detail_falls_back_to_first_error_without_trigger() {
        use crate::types::{EventKind, TaskEvent};
        let now = Local::now();
        let events = vec![
            TaskEvent {
                task_id: TaskId("t-fallback".to_string()),
                timestamp: now,
                event_kind: EventKind::Error,
                detail: "first generic error".to_string(),
                metadata: None,
            },
            TaskEvent {
                task_id: TaskId("t-fallback".to_string()),
                timestamp: now,
                event_kind: EventKind::Error,
                detail: "second generic error".to_string(),
                metadata: None,
            },
        ];

        assert_eq!(last_error_detail(&events).as_deref(), Some("first generic error"));
    }

    #[test]
    fn detail_output_parses_log_jsonl_content() {
        let log_file = NamedTempFile::new().unwrap();
        std::fs::write(
            log_file.path(),
            concat!(
                "not-json-prefix\n",
                "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"hello\"}}\n",
                "{\"type\":\"message\",\"role\":\"assistant\",\"content\":\" world\",\"delta\":true}\n"
            ),
        )
        .unwrap();
        let mut task = make_task();
        task.log_path = Some(log_file.path().display().to_string());

        assert_eq!(read_task_output_for_tui(&task), "hello\n---\n world");
    }
}
