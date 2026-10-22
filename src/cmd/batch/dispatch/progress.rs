// Completion progress labels for batch dispatch.
// Deps: task records, store, and batch outcomes.
use super::*;

pub(crate) fn completion_progress_label(
    store: &Store,
    task_id: &str,
    outcome: BatchTaskOutcome,
) -> Result<String> {
    let task = store.get_task(task_id)?;
    Ok(match outcome {
        BatchTaskOutcome::Done => format!("DONE ({})", progress_duration(task.as_ref())),
        BatchTaskOutcome::Failed => format!("FAIL ({})", failure_reason(task.as_ref())),
        BatchTaskOutcome::Skipped => "SKIP".to_string(),
    })
}

fn progress_duration(task: Option<&Task>) -> String {
    let Some(duration_ms) = task.and_then(|task| task.duration_ms) else {
        return "unknown".to_string();
    };
    if duration_ms < 1_000 {
        return format!("{duration_ms}ms");
    }
    let secs = duration_ms / 1_000;
    let tenths = (duration_ms % 1_000) / 100;
    if tenths == 0 {
        format!("{secs}s")
    } else {
        format!("{secs}.{tenths}s")
    }
}

fn failure_reason(task: Option<&Task>) -> String {
    if let Some(reason) = task.and_then(|task| task.pending_reason.as_deref()) {
        return reason.to_string();
    }
    if let Some(exit_code) = task.and_then(|task| task.exit_code) {
        return format!("exit {exit_code}");
    }
    "failed".to_string()
}
