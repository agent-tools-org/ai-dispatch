// Retry-chain formatting for task details.
// Uses board duration formatting and task status labels.

use super::{Task, TaskStatus, elapsed_since, format_duration_ms};
use crate::cost;

fn retry_kind(task: &Task) -> &'static str {
    if task.parent_task_id.is_some() {
        "retry"
    } else {
        "root"
    }
}

fn retry_status(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Waiting => "Waiting",
        TaskStatus::Pending => "Pending",
        TaskStatus::Running => "Running",
        TaskStatus::AwaitingInput => "Await",
        TaskStatus::Stalled => "Stalled",
        TaskStatus::Done => "Done",
        TaskStatus::Merged => "Merged",
        TaskStatus::Failed => "Failed",
        TaskStatus::Skipped => "Skipped",
        TaskStatus::Stopped => "Stopped",
    }
}

pub(super) fn append_retry_chain(out: &mut String, task: &Task, retry_chain: Option<&[Task]>) {
    if let Some(parent) = task.parent_task_id.as_deref() {
        out.push_str(&format!("Parent: {parent}\n"));
        if let Some(retry_chain) = retry_chain
            && retry_chain.len() > 1
        {
            out.push_str("Retry chain:\n");
            for retry_task in retry_chain {
                let duration = retry_task
                    .duration_ms
                    .map(format_duration_ms)
                    .unwrap_or_else(|| elapsed_since(retry_task.created_at));
                let current = if retry_task.id == task.id {
                    "  ← current"
                } else {
                    ""
                };
                out.push_str(&format!(
                    "  {} ({})  → {:<7} {:>5}  {}{}\n",
                    retry_task.id,
                    retry_kind(retry_task),
                    retry_status(retry_task.status),
                    duration,
                    cost::format_cost(retry_task.cost_usd),
                    current,
                ));
            }
        }
    }
}
