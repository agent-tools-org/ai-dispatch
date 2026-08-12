// Text rendering for task board and task detail views.
// Board rows can enrich output with stored milestone events.

use anyhow::Result;

use crate::cmd::eta;
use crate::cost;
use crate::store::Store;
use crate::types::*;

mod detail;
mod rows;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod outcome_tests;
#[cfg(test)]
mod detail_tests;

pub use detail::render_task_detail;

/// Render a summary table of tasks (for `aid board`)
pub fn render_board(tasks: &[Task], store: &Store) -> Result<String> {
    if tasks.is_empty() {
        return Ok("No tasks found.".to_string());
    }

    let (done, running, failed) = count_statuses(tasks);
    let total_tokens: i64 = tasks.iter().filter_map(|t| t.tokens).sum();
    let total_cost: f64 = tasks.iter().filter_map(|t| t.cost_usd).sum();

    let mut out = String::new();
    out.push_str(&format!(
        "Tasks: {} total | {} done | {} running | {} failed\n",
        tasks.len(), done, running, failed,
    ));
    if total_tokens > 0 {
        out.push_str(&format!("Total tokens: {}", format_tokens(total_tokens)));
        if total_cost > 0.0 {
            out.push_str(&format!("  Cost: {}", cost::format_cost(Some(total_cost))));
        }
        out.push('\n');
    }
    out.push('\n');

    let show_repo = tasks.iter().any(|task| task.repo_path.is_some());
    let running_ids: Vec<&str> = tasks
        .iter()
        .filter(|task| task.status != TaskStatus::AwaitingInput)
        .map(|task| task.id.as_str())
        .collect();
    let awaiting_ids: Vec<&str> = tasks
        .iter()
        .filter(|task| task.status == TaskStatus::AwaitingInput)
        .map(|task| task.id.as_str())
        .collect();
    let latest_milestones = store.latest_milestones_batch(&running_ids)?;
    let awaiting_reasons = store.latest_awaiting_reasons_batch(&awaiting_ids)?;
    let failed_ids: Vec<&str> = tasks
        .iter()
        .filter(|task| task.status == TaskStatus::Failed)
        .map(|task| task.id.as_str())
        .collect();
    let latest_errors = store.latest_errors_batch(&failed_ids)?;
    // Fallback: for tasks without a filtered error, use any latest error
    let missing_error_ids: Vec<&str> = failed_ids.iter()
        .filter(|id| !latest_errors.contains_key(**id))
        .copied()
        .collect();
    let fallback_errors = store.latest_errors_batch_unfiltered(&missing_error_ids)?;
    let latest_errors = {
        let mut merged = latest_errors;
        merged.extend(fallback_errors);
        merged
    };

    // Header — Route is cli/provider/model (attribution rides on the model segment).
    // Project is first-class; Group remains for workgroup scope.
    if show_repo {
        out.push_str(&format!(
            "{:<11} {:<36} {:<30} {:<10} {:<10} {:<8} {:<11} {:<14} {:<12} {:<20} {:<16} {}\n",
            "ID", "Route", "Status", "Duration", "Tokens", "Cost", "Parent", "Project", "Group", "Repo", "Caller", "Model"
        ));
        out.push_str(&"-".repeat(206));
        out.push('\n');
    } else {
        out.push_str(&format!(
            "{:<11} {:<36} {:<30} {:<10} {:<10} {:<8} {:<11} {:<14} {:<12} {:<16} {}\n",
            "ID", "Route", "Status", "Duration", "Tokens", "Cost", "Parent", "Project", "Group", "Caller", "Model"
        ));
        out.push_str(&"-".repeat(185));
        out.push('\n');
    }

    rows::append_grouped_task_rows(
        &mut out,
        tasks,
        store,
        show_repo,
        &awaiting_reasons,
        &latest_milestones,
        &latest_errors,
    );
    Ok(out)
}

fn count_statuses(tasks: &[Task]) -> (usize, usize, usize) {
    let mut done = 0;
    let mut running = 0;
    let mut failed = 0;
    for t in tasks {
        match t.outcome() {
            TaskOutcome::Verified | TaskOutcome::Delivered => done += 1,
            TaskOutcome::InProgress if matches!(t.status, TaskStatus::Running | TaskStatus::AwaitingInput | TaskStatus::Stalled) => running += 1,
            TaskOutcome::InProgress => {}
            TaskOutcome::Broken | TaskOutcome::Failed | TaskOutcome::Stopped => failed += 1,
            TaskOutcome::Unverified(_) | TaskOutcome::Skipped => {}
        }
    }
    (done, running, failed)
}

pub(super) fn format_with_outcome(task: &Task, base: String) -> String {
    task.outcome()
        .verification_tag()
        .map(|tag| format!("{base} [{tag}]"))
        .unwrap_or(base)
}

pub(super) fn format_duration(ms: i64) -> String {
    let secs = ms / 1000;
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}

fn elapsed_since(start: chrono::DateTime<chrono::Local>) -> String {
    let elapsed = chrono::Local::now() - start;
    let secs = elapsed.num_seconds();
    if secs < 0 {
        "0s".to_string()
    } else if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}

pub(super) fn format_running_duration(task: &Task, store: &Store) -> String {
    let elapsed = elapsed_since(task.created_at);
    match (eta::estimate_eta(task, store), eta::estimate_progress(task, store)) {
        (Some(eta_label), Some(progress)) => format!("{elapsed} (ETA {eta_label} {progress}%)"),
        (Some(eta_label), None) => format!("{elapsed} (ETA {eta_label})"),
        (None, Some(progress)) => format!("{elapsed} ({progress}%)"),
        (None, None) => elapsed,
    }
}

pub(super) fn format_tokens(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

pub(super) fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let safe = s.floor_char_boundary(max.saturating_sub(3));
        format!("{}...", &s[..safe])
    }
}

pub(super) fn short_parent(parent: Option<&str>) -> String {
    parent.unwrap_or("-").to_string()
}

pub(super) fn short_group(group: Option<&str>) -> String {
    group.unwrap_or("-").to_string()
}

pub(super) fn short_repo(repo: Option<&str>) -> String {
    repo.map(|path| truncate(path, 20))
        .unwrap_or_else(|| "-".to_string())
}

pub(super) fn task_status(task: &Task, milestone: Option<String>, latest_error: Option<String>) -> String {
    let base = if task.status == TaskStatus::Failed {
        if let Some(pending_reason) = task.pending_reason.as_deref() {
            truncate(&format!("{} — {}", task.status.label(), pending_reason), 30)
        } else if let Some(error) = latest_error {
            truncate(&format!("{} — {}", task.status.label(), error), 30)
        } else {
            task.status.label().to_string()
        }
    } else if task.status == TaskStatus::Running
        && let Some(milestone) = milestone
    {
        truncate(&format!("{} — {}", task.status.label(), milestone), 30)
    } else {
        task.status.label().to_string()
    };
    with_delivery_suffix(task, base)
}

fn with_delivery_suffix(task: &Task, base: String) -> String {
    if !matches!(task.status, TaskStatus::Done | TaskStatus::Failed | TaskStatus::Stopped) {
        return base;
    }
    let Some(delivery) = task.delivery_assessment() else {
        return base;
    };
    if !delivery.implies_no_changes() {
        return base;
    }
    format!("{base} [delivery:{}]", delivery.as_str())
}
