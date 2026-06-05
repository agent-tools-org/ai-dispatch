// Text rendering for task board and task detail views.
// Board rows can enrich output with stored milestone events.

use std::collections::HashMap;

use anyhow::Result;

use crate::cmd::eta;
use crate::cost;
use crate::session;
use crate::store::Store;
use crate::types::*;

mod detail;
#[cfg(test)]
mod tests;

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

    // Header
    if show_repo {
        out.push_str(&format!(
            "{:<11} {:<10} {:<30} {:<10} {:<10} {:<8} {:<10} {:<12} {:<20} {:<16} {}\n",
            "ID", "Agent", "Status", "Duration", "Tokens", "Cost", "Parent", "Group", "Repo", "Caller", "Model"
        ));
        out.push_str(&"-".repeat(165));
        out.push('\n');
    } else {
        out.push_str(&format!(
            "{:<11} {:<10} {:<30} {:<10} {:<10} {:<8} {:<10} {:<12} {:<16} {}\n",
            "ID", "Agent", "Status", "Duration", "Tokens", "Cost", "Parent", "Group", "Caller", "Model"
        ));
        out.push_str(&"-".repeat(144));
        out.push('\n');
    }

    for task in tasks {
        let status = if task.status == TaskStatus::AwaitingInput {
            let reason = awaiting_reasons.get(task.id.as_str());
            match reason {
                Some(r) => truncate(&format!("AWAIT — {}", r), 30),
                None => task.status.label().to_string(),
            }
        } else {
            let error = latest_errors.get(task.id.as_str()).cloned();
            let base = task_status(task, latest_milestone(&latest_milestones, task.id.as_str()), error);
            if task.has_verify_failure() {
                format!("{} [VFAIL]", base)
            } else {
                base
            }
        };
        let duration = if task.status == TaskStatus::Skipped {
            "-".to_string()
        } else {
            task.duration_ms
                .map(format_duration)
                .unwrap_or_else(|| format_running_duration(task, store))
        };
        let tokens = if task.status == TaskStatus::Skipped {
            "-".to_string()
        } else {
            task.tokens
                .map(format_tokens)
                .unwrap_or_else(|| "-".to_string())
        };
        let cost_str = if task.status == TaskStatus::Skipped {
            "-".to_string()
        } else {
            cost::format_cost_label(task.cost_usd, task.agent)
        };
        let parent = short_parent(task.parent_task_id.as_deref());
        let group = short_group(task.workgroup_id.as_deref());
        let repo = short_repo(task.repo_path.as_deref());
        let caller = session::display(task);
        let model = task.model
            .as_deref()
            .unwrap_or("-");

        if show_repo {
            out.push_str(&format!(
                "{:<11} {:<10} {:<30} {:<10} {:<10} {:<8} {:<10} {:<12} {:<20} {:<16} {}\n",
                task.id.as_str(),
                task.agent_display_name(),
                status,
                duration,
                tokens,
                cost_str,
                parent,
                group,
                repo,
                caller,
                model,
            ));
        } else {
            out.push_str(&format!(
                "{:<11} {:<10} {:<30} {:<10} {:<10} {:<8} {:<10} {:<12} {:<16} {}\n",
                task.id.as_str(),
                task.agent_display_name(),
                status,
                duration,
                tokens,
                cost_str,
                parent,
                group,
                caller,
                model,
            ));
        }
    }
    Ok(out)
}

fn latest_milestone(milestones: &HashMap<String, String>, task_id: &str) -> Option<String> {
    milestones.get(task_id).cloned()
}

fn count_statuses(tasks: &[Task]) -> (usize, usize, usize) {
    let mut done = 0;
    let mut running = 0;
    let mut failed = 0;
    for t in tasks {
        match t.status {
            TaskStatus::Done | TaskStatus::Merged => done += 1,
            TaskStatus::Running | TaskStatus::AwaitingInput | TaskStatus::Stalled => running += 1,
            TaskStatus::Failed | TaskStatus::Stopped => failed += 1,
            TaskStatus::Pending | TaskStatus::Waiting | TaskStatus::Skipped => {}
        }
    }
    (done, running, failed)
}

fn format_duration(ms: i64) -> String {
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

fn format_running_duration(task: &Task, store: &Store) -> String {
    let elapsed = elapsed_since(task.created_at);
    match (eta::estimate_eta(task, store), eta::estimate_progress(task, store)) {
        (Some(eta_label), Some(progress)) => format!("{elapsed} (ETA {eta_label} {progress}%)"),
        (Some(eta_label), None) => format!("{elapsed} (ETA {eta_label})"),
        (None, Some(progress)) => format!("{elapsed} ({progress}%)"),
        (None, None) => elapsed,
    }
}

fn format_tokens(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let safe = s.floor_char_boundary(max.saturating_sub(3));
        format!("{}...", &s[..safe])
    }
}

fn short_parent(parent: Option<&str>) -> String {
    parent.unwrap_or("-").to_string()
}

fn short_group(group: Option<&str>) -> String {
    group.unwrap_or("-").to_string()
}

fn short_repo(repo: Option<&str>) -> String {
    repo.map(|path| truncate(path, 20))
        .unwrap_or_else(|| "-".to_string())
}

fn task_status(task: &Task, milestone: Option<String>, latest_error: Option<String>) -> String {
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
