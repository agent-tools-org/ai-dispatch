// Text rendering for task board and task detail views.
// Board rows can enrich output with stored milestone events.

use anyhow::Result;

use crate::cmd::eta;
use crate::cost;
use crate::store::Store;
use crate::types::*;
use crate::ui::format::format_duration_ms;
use crate::ui::format::format_tokens;
use crate::ui::format::truncate;

mod detail;
#[cfg(test)]
mod detail_tests;
#[cfg(test)]
mod outcome_tests;
mod rows;
#[cfg(test)]
mod tests;

pub use detail::render_task_detail;

/// Render a summary table of tasks (for `aid board`)
pub fn render_board(tasks: &[Task], store: &Store) -> Result<String> {
    if tasks.is_empty() {
        return Ok("No tasks found.".to_string());
    }

    let mut out = board_summary(tasks);

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
    let latest_errors = load_latest_errors(tasks, store)?;

    append_column_header(&mut out, show_repo);

    rows::append_grouped_task_rows(
        &mut out,
        tasks,
        store,
        show_repo,
        &awaiting_reasons,
        &latest_milestones,
        &latest_errors,
    );
    append_infrastructure_diagnostics(&mut out, tasks, store)?;
    Ok(out)
}

fn append_infrastructure_diagnostics(
    out: &mut String,
    tasks: &[Task],
    store: &Store,
) -> Result<()> {
    for task in tasks
        .iter()
        .filter(|task| task.verify_status == VerifyStatus::InfrastructureFailure)
    {
        if let Some(event) = store
            .get_events(task.id.as_str())?
            .iter()
            .rev()
            .find(|event| {
                event
                    .metadata
                    .as_ref()
                    .is_some_and(|meta| meta["verify_execution_error"] == true)
            })
        {
            out.push_str(&format!("{} [VINFRA] {}\n", task.id, event.detail));
        }
    }
    Ok(())
}

fn count_statuses(tasks: &[Task]) -> (usize, usize, usize) {
    let mut done = 0;
    let mut running = 0;
    let mut failed = 0;
    for t in tasks {
        match t.outcome() {
            TaskOutcome::Verified | TaskOutcome::Delivered => done += 1,
            TaskOutcome::InProgress
                if matches!(
                    t.status,
                    TaskStatus::Running | TaskStatus::AwaitingInput | TaskStatus::Stalled
                ) =>
            {
                running += 1
            }
            TaskOutcome::InProgress => {}
            TaskOutcome::Broken | TaskOutcome::Failed => failed += 1,
            TaskOutcome::Unverified(_) | TaskOutcome::Skipped | TaskOutcome::Stopped => {}
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
    match (
        eta::estimate_eta(task, store),
        eta::estimate_progress(task, store),
    ) {
        (Some(eta_label), Some(progress)) => format!("{elapsed} (ETA {eta_label} {progress}%)"),
        (Some(eta_label), None) => format!("{elapsed} (ETA {eta_label})"),
        (None, Some(progress)) => format!("{elapsed} ({progress}%)"),
        (None, None) => elapsed,
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

pub(super) fn task_status(
    task: &Task,
    milestone: Option<String>,
    latest_error: Option<String>,
) -> String {
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
    if !matches!(
        task.status,
        TaskStatus::Done | TaskStatus::Failed | TaskStatus::Stopped
    ) {
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

fn append_column_header(out: &mut String, show_repo: bool) {
    // Header — Route is cli/provider/model (attribution rides on the model segment).
    // Project is first-class; Group remains for workgroup scope.
    if show_repo {
        out.push_str(&format!(
            "{:<11} {:<36} {:<30} {:<10} {:<10} {:<8} {:<11} {:<14} {:<12} {:<20} {:<16} {}\n",
            "ID",
            "Route",
            "Status",
            "Duration",
            "Tokens",
            "Cost",
            "Parent",
            "Project",
            "Group",
            "Repo",
            "Caller",
            "Model"
        ));
        out.push_str(&"-".repeat(206));
        out.push('\n');
    } else {
        out.push_str(&format!(
            "{:<11} {:<36} {:<30} {:<10} {:<10} {:<8} {:<11} {:<14} {:<12} {:<16} {}\n",
            "ID",
            "Route",
            "Status",
            "Duration",
            "Tokens",
            "Cost",
            "Parent",
            "Project",
            "Group",
            "Caller",
            "Model"
        ));
        out.push_str(&"-".repeat(185));
        out.push('\n');
    }
}

fn board_summary(tasks: &[Task]) -> String {
    let (done, running, failed) = count_statuses(tasks);
    let total_tokens: i64 = tasks.iter().filter_map(|t| t.tokens).sum();
    let total_cost: f64 = tasks.iter().filter_map(|t| t.cost_usd).sum();
    let unknown_cost = cost::unknown_cost_tasks(tasks);

    let mut out = String::new();
    out.push_str(&format!(
        "Tasks: {} total | {} done | {} running | {} failed\n",
        tasks.len(),
        done,
        running,
        failed,
    ));
    if total_tokens > 0 {
        out.push_str(&format!("Total tokens: {}", format_tokens(total_tokens)));
        if total_cost > 0.0 || unknown_cost > 0 {
            out.push_str(&format!(
                "  Cost: {}",
                cost::format_cost_total(total_cost, unknown_cost)
            ));
        }
        out.push('\n');
    }
    out.push('\n');

    out
}

fn load_latest_errors(
    tasks: &[Task],
    store: &Store,
) -> Result<std::collections::HashMap<String, String>> {
    let failed_ids: Vec<&str> = tasks
        .iter()
        .filter(|task| task.status == TaskStatus::Failed)
        .map(|task| task.id.as_str())
        .collect();
    let latest_errors = store.latest_errors_batch(&failed_ids)?;
    // Fallback: for tasks without a filtered error, use any latest error
    let missing_error_ids: Vec<&str> = failed_ids
        .iter()
        .filter(|id| !latest_errors.contains_key(**id))
        .copied()
        .collect();
    let fallback_errors = store.latest_errors_batch_unfiltered(&missing_error_ids)?;
    let latest_errors = {
        let mut merged = latest_errors;
        merged.extend(fallback_errors);
        merged
    };
    Ok(latest_errors)
}
