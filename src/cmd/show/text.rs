// Text views for aid show, including stored dispatch context and task status.
// Exports context_text and summary_text; shares task and output helpers with show.
// Other rendering paths retain their existing behavior.
use super::*;

pub(super) fn render_output_text(
    store: &Arc<Store>,
    task_id: &str,
    full: bool,
    brief: bool,
) -> Result<String> {
    if full && !brief {
        return output_text(store, task_id);
    }
    render_output_brief_text(store, task_id)
}

pub(super) fn render_output_brief_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let mut text = output_text_brief(store, task_id)?;
    let truncated = output_text(store, task_id)
        .map(|full_text| full_text != text)
        .unwrap_or(false);
    if truncated {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!(
            "[truncated — use `aid show {task_id} --output --full` for full content]"
        ));
    }
    Ok(text)
}

pub(super) fn render_log_text(task_id: &str, full: bool) -> Result<String> {
    if full {
        return log_text(task_id);
    }
    log_text_brief(task_id)
}

pub(super) fn result_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let path = crate::paths::task_dir(task_id).join("result.md");
    let banner = load_task(store, task_id)
        .ok()
        .and_then(|task| audit_result_missing_banner(store, &task));
    if !path.exists() {
        return Ok(banner.unwrap_or_else(|| "No result file for this task\n".to_string()));
    }
    let body = std::fs::read_to_string(path)?;
    // The banner fires here only when the file was salvaged from the log, so keep the
    // salvaged text visible but never let it be read as the report on its own.
    Ok(match banner {
        Some(banner) => format!("{banner}\n{body}"),
        None => body,
    })
}

pub(super) fn events_text(store: &Arc<Store>, task_id: &str, full: bool) -> Result<String> {
    let events = store.get_events(task_id)?;
    if events.is_empty() {
        return Ok("No events.\n".to_string());
    }
    let mut out = String::from("Events:\n");
    for event in events {
        let detail = if full {
            event.full_detail()
        } else {
            &event.detail
        };
        out.push_str(&format!(
            "  {}  [{:>10}] {}\n",
            event.timestamp.format("%H:%M:%S"),
            event.event_kind.as_str(),
            detail,
        ));
    }
    Ok(out)
}

/// When an audit-style task ends without producing the structured `result.md`,
/// surface a clear retry hint instead of letting the caller fall back to the
/// raw (often truncated) agent output.
pub(super) fn audit_result_missing_banner(
    store: &Store,
    task: &crate::types::Task,
) -> Option<String> {
    if !matches!(task.status, TaskStatus::Done | TaskStatus::Merged) {
        return None;
    }
    // Only a task that was asked for a result file can be missing one; prompt wording is no evidence.
    let requested = crate::cmd::run::RunArgs::saved_for_task(store, &task.id.0)
        .ok()
        .flatten()
        .is_some_and(|args| args.result_file.is_some());
    if !requested {
        return None;
    }
    // A result.md salvaged from the log is not a delivered report, so the delivery
    // assessment recorded at persist time outranks the file merely existing.
    let missing_delivery =
        task.delivery_assessment() == Some(crate::types::DeliveryAssessment::MissingFinalDelivery);
    let result_path = crate::paths::task_dir(&task.id.0).join("result.md");
    if result_path.exists() && !missing_delivery {
        return None;
    }
    Some(format!(
        "⚠ Structured audit result missing.\n\
         The agent did not write result.md as instructed.\n\
         Likely cause: the agent ended its turn mid-investigation, or ignored the result-file instruction.\n\
         Anything shown below is captured tool output, not a report.\n\
         Retry with a stronger agent:\n  \
         aid retry {} --agent codex\n",
        task.id.0
    ))
}

pub(super) fn transcript_text(task_id: &str) -> Result<String> {
    let path = crate::paths::transcript_path(task_id);
    std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read transcript {}", path.display()))
}

// --- Default mode: events + stderr + diff stat ---

pub(super) fn terminal_missing_result_notice(task: &crate::types::Task) -> Option<String> {
    if !matches!(task.status, TaskStatus::Done | TaskStatus::Failed) {
        return None;
    }
    let result_path = crate::paths::task_dir(task.id.as_str()).join("result.md");
    if result_path.exists() {
        return None;
    }
    if task.status == TaskStatus::Done {
        return Some(format!(
            "Status: {} (no result file - see --output / output.md)\n",
            human_status(task)
        ));
    }
    Some(format!("Status: {}\n", human_status(task)))
}

pub(super) fn human_status(task: &Task) -> String {
    task.outcome()
        .verification_tag()
        .map(|tag| format!("{} [{tag}]", show_status_label(task)))
        .unwrap_or_else(|| show_status_label(task).to_string())
}

pub(super) fn show_status_label(task: &Task) -> &'static str {
    match task.status {
        TaskStatus::Failed => "FAILED",
        status => status.label(),
    }
}

pub fn summary_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = load_task(store, task_id)?;
    let mut out = summary_header(&task);
    let mut live_dirty = false;
    if let Some(ref wt_path) = task.worktree_path
        && Path::new(wt_path).exists()
    {
        let state = crate::worktree::capture_live_worktree_state(Path::new(wt_path)).ok();
        live_dirty = state.as_ref().is_some_and(|state| state.is_dirty());
        out.push_str(&worktree_state_section(wt_path, state.as_ref()));
    }
    if task
        .delivery_assessment()
        .is_some_and(|delivery| delivery.implies_no_changes())
        && !live_dirty
    {
        out.push_str("\n--- Diff Stat ---\n  (no changes detected)\n");
    } else if let Some(ref wt_path) = task.worktree_path
        && Path::new(wt_path).exists()
    {
        out.push_str("\n--- Diff Stat ---\n");
        let base_ref = branch_base_ref(wt_path);
        let stat = diff_stat(wt_path, task.start_sha.as_deref(), base_ref.as_deref());
        if live_dirty && stat.contains("(no changes detected)") {
            out.push_str("  (no committed diff detected)\n");
        } else {
            out.push_str(&stat);
        }
    } else if task.worktree_branch.is_none()
        && matches!(task.status, TaskStatus::Done | TaskStatus::Merged)
    {
        let repo = task.repo_path.as_deref().unwrap_or(".");
        out.push_str("\n--- Diff Stat ---\n");
        if let Some(stat) = inplace_diff_stat(repo) {
            out.push_str(&stat);
        } else {
            out.push_str("  (no changes detected)\n");
        }
    }

    if !out.contains("--- Diff Stat ---") || out.contains("(no changes detected)") {
        if let Some(conclusion) = completion_conclusion(store.as_ref(), task.id.as_str()) {
            out.push_str("\nConclusion: ");
            out.push_str(&conclusion);
            out.push('\n');
        }
    }

    Ok(out)
}

pub fn context_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = load_task(store, task_id)?;
    let mut out = String::new();
    out.push_str("=== Original Prompt ===\n");
    out.push_str(&task.prompt);
    out.push('\n');

    if let Some(resolved_prompt) = task.resolved_prompt.as_deref() {
        out.push_str("\n=== Resolved Prompt ===\n");
        out.push_str(resolved_prompt);
        out.push('\n');
        return Ok(out);
    }

    out.push_str("\n=== Resolved Prompt ===\n(not stored for this task)\n");
    Ok(out)
}

fn summary_header(task: &Task) -> String {
    let mut out = String::new();
    out.push_str(&format!("=== Review: {} ===\n", task.id));
    out.push_str(&format!(
        "Route: {}  Status: {}  Prompt: {}\n",
        task.display_route(),
        human_status(task),
        task.prompt,
    ));
    if let Some(verdict) = task.audit_verdict.as_deref() {
        out.push_str("Audit: ");
        out.push_str(verdict);
        if let Some(report_path) = task.audit_report_path.as_deref() {
            out.push_str(&format!(" (report: {report_path})"));
        }
        out.push('\n');
    }

    out
}
