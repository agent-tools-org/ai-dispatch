// Shared one-line status bar for every task-oriented TUI view.
// Exports: `render_status_bar`; derives counts and resource totals from App state.
// Deps: App, ProcessMetrics, TaskStatus, and ratatui Paragraph rendering.

use std::collections::HashMap;

use ratatui::prelude::{Color, Style};
use ratatui::widgets::Paragraph;

use super::app::App;
use super::metrics::ProcessMetrics;
use crate::types::{Task, TaskStatus};

#[derive(Clone, Copy)]
pub(crate) enum StatusBarMode {
    Board,
    Dashboard,
    Stats,
    Multipane { extra_panes: usize },
}

#[derive(Debug, PartialEq)]
struct TaskCounts {
    total: usize,
    running: usize,
    failed: usize,
}

#[derive(Debug, PartialEq)]
struct ResourceTotals {
    cpu_percent: f32,
    memory_mb: f32,
}

#[derive(Debug, PartialEq)]
struct StatusSnapshot {
    counts: TaskCounts,
    resources: ResourceTotals,
    scope: String,
}

pub(crate) fn render_status_bar(
    frame: &mut ratatui::Frame<'_>,
    area: ratatui::layout::Rect,
    app: &App,
    mode: StatusBarMode,
) {
    let text = build_status_text(&snapshot(app), area.width as usize, mode);
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(Color::Indexed(243))),
        area,
    );
}

fn snapshot(app: &App) -> StatusSnapshot {
    StatusSnapshot {
        counts: count_tasks(&app.tasks),
        resources: sum_metrics(&app.metrics),
        scope: app.scope_label(),
    }
}

fn count_tasks(tasks: &[Task]) -> TaskCounts {
    let mut counts = TaskCounts {
        total: 0,
        running: 0,
        failed: 0,
    };
    for task in tasks {
        counts.total += 1;
        if matches!(
            task.status,
            TaskStatus::Running | TaskStatus::AwaitingInput | TaskStatus::Stalled
        ) {
            counts.running += 1;
        }
        if matches!(task.status, TaskStatus::Failed | TaskStatus::Stopped) {
            counts.failed += 1;
        }
    }
    counts
}

fn sum_metrics(metrics: &HashMap<String, ProcessMetrics>) -> ResourceTotals {
    metrics.values().fold(
        ResourceTotals {
            cpu_percent: 0.0,
            memory_mb: 0.0,
        },
        |mut totals, metrics| {
            totals.cpu_percent += metrics.cpu_percent;
            totals.memory_mb += metrics.memory_mb;
            totals
        },
    )
}

fn build_status_text(snapshot: &StatusSnapshot, width: usize, mode: StatusBarMode) -> String {
    let metrics = [
        format!("Running {}/{}", snapshot.counts.running, snapshot.counts.total),
        format!("CPU {:.1}%", snapshot.resources.cpu_percent),
        format!("RAM {:.0}M", snapshot.resources.memory_mb),
        format!("Failed {}", snapshot.counts.failed),
        format!("Scope {}", snapshot.scope),
    ];
    let full = fit_line(&metrics, mode_hint(mode), width);
    if display_width(&full) <= width {
        return full;
    }

    let compact = [
        format!("Run {}/{}", snapshot.counts.running, snapshot.counts.total),
        format!("CPU {:.1}%", snapshot.resources.cpu_percent),
        format!("RAM {:.0}M", snapshot.resources.memory_mb),
        format!("Fail {}", snapshot.counts.failed),
        format!("Scope {}", snapshot.scope),
    ];
    let compact_line = compact.join(" ");
    if display_width(&compact_line) <= width {
        return compact_line;
    }

    let prefix = format!(
        "R {}/{} C {:.1}% M {:.0}M F {} S ",
        snapshot.counts.running,
        snapshot.counts.total,
        snapshot.resources.cpu_percent,
        snapshot.resources.memory_mb,
        snapshot.counts.failed
    );
    if display_width(&prefix) >= width {
        return truncate(&prefix, width);
    }
    format!(
        "{}{}",
        prefix,
        truncate(&snapshot.scope, width - display_width(&prefix))
    )
}

fn fit_line(metrics: &[String; 5], hint: String, width: usize) -> String {
    let line = format!("{}  | {}", metrics.join("  "), hint);
    if display_width(&line) <= width {
        line
    } else {
        metrics.join("  ")
    }
}

fn display_width(value: &str) -> usize {
    value.chars().count()
}

fn mode_hint(mode: StatusBarMode) -> String {
    match mode {
        StatusBarMode::Board | StatusBarMode::Dashboard => {
            "a=all/today s=stats d=dashboard m=multipane j/k=nav Enter=detail q=quit".into()
        }
        StatusBarMode::Stats => "a=all/today s=stats v=legacy d=dashboard m=multipane q=quit".into(),
        StatusBarMode::Multipane { extra_panes } => {
            let extra = (extra_panes > 0).then(|| format!(" | +{extra_panes} more"));
            format!(
                "Tab=pane j/k PgUp/PgDn Home/End Enter=detail Esc=board q=quit{}",
                extra.unwrap_or_default()
            )
        }
    }
}

fn truncate(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_string();
    }
    if width <= 3 {
        return value.chars().take(width).collect();
    }
    let prefix: String = value.chars().take(width - 3).collect();
    format!("{prefix}...")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use crate::types::{AgentKind, TaskId, VerifyStatus};

    fn task(status: TaskStatus) -> Task {
        Task {
            id: TaskId("t-status".to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "status test".to_string(),
            resolved_prompt: None,
            category: None,
            status,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None, project_id: crate::project::current_project_id(),
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
    fn counts_tasks_in_one_pass() {
        let tasks = [
            task(TaskStatus::Running),
            task(TaskStatus::AwaitingInput),
            task(TaskStatus::Done),
            task(TaskStatus::Failed),
            task(TaskStatus::Stopped),
        ];

        assert_eq!(
            count_tasks(&tasks),
            TaskCounts {
                total: 5,
                running: 2,
                failed: 2,
            }
        );
    }

    #[test]
    fn narrows_status_bar_to_compact_literal_without_wrapping() {
        let snapshot = StatusSnapshot {
            counts: TaskCounts {
                total: 8,
                running: 2,
                failed: 1,
            },
            resources: ResourceTotals {
                cpu_percent: 12.4,
                memory_mb: 256.0,
            },
            scope: "today+active".to_string(),
        };

        assert_eq!(
            build_status_text(&snapshot, 32, StatusBarMode::Dashboard),
            "R 2/8 C 12.4% M 256M F 1 S to..."
        );
    }
}
