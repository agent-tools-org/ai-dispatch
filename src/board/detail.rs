// Detailed task board rendering for audit-style views.
// Exports render_task_detail for crate::board.
// Deps: parent board formatting helpers and task/event types.

use crate::cost;
use crate::session;
use crate::types::{Task, TaskEvent, TaskStatus};

use super::{elapsed_since, format_duration, format_tokens, truncate};

/// Render detailed view of a single task (for `aid audit`)
pub fn render_task_detail(task: &Task, events: &[TaskEvent], retry_chain: Option<Vec<Task>>) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "Task: {} — {}: {}\n",
        task.id,
        task.agent_display_name(),
        truncate(&task.prompt, 60),
    ));

    let duration = task.duration_ms
        .map(format_duration)
        .unwrap_or_else(|| elapsed_since(task.created_at));
    out.push_str(&format!("Status: {}  Duration: {}\n", task.status.label(), duration));
    if let Some(pending_reason) = task.pending_reason.as_deref() {
        out.push_str(&format!("Pending reason: {pending_reason}\n"));
    }
    if let Some(parent) = task.parent_task_id.as_deref() {
        out.push_str(&format!("Parent: {parent}\n"));
        if let Some(retry_chain) = retry_chain.as_deref()
            && retry_chain.len() > 1
        {
            out.push_str("Retry chain:\n");
            for retry_task in retry_chain {
                let duration = retry_task.duration_ms
                    .map(format_duration)
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
    append_task_metadata(&mut out, task);
    append_task_events(&mut out, events);
    out
}

fn append_task_metadata(out: &mut String, task: &Task) {
    if let Some(group_id) = task.workgroup_id.as_deref() {
        out.push_str(&format!("Workgroup: {group_id}\n"));
    }
    if let Some(repo_path) = task.repo_path.as_deref() {
        out.push_str(&format!("Repo: {repo_path}\n"));
    }
    if task.caller_kind.is_some() || task.caller_session_id.is_some() {
        out.push_str(&format!("Caller: {}\n", session::display(task)));
    }
    if let Some(tokens) = task.tokens {
        out.push_str(&format!("Tokens: {}", format_tokens(tokens)));
        if let Some(c) = task.cost_usd {
            out.push_str(&format!("  Cost: {}", cost::format_cost(Some(c))));
        }
        out.push('\n');
    }
    if let Some(prompt_tokens) = task.prompt_tokens {
        let bytes = task.resolved_prompt.as_deref().map(|p| p.len()).unwrap_or(0);
        out.push_str(&format!("Prompt: ~{} tokens ({} bytes)\n", prompt_tokens, bytes));
    }
    append_paths_and_audit(out, task);
}

fn append_paths_and_audit(out: &mut String, task: &Task) {
    if let Some(ref wt) = task.worktree_path {
        out.push_str(&format!("Worktree: {}", wt));
        if let Some(ref branch) = task.worktree_branch {
            out.push_str(&format!(" ({})", branch));
        }
        out.push('\n');
    }
    if let Some(ref log) = task.log_path {
        out.push_str(&format!("Log: {}\n", log));
    }
    if let Some(ref output) = task.output_path {
        out.push_str(&format!("Output: {}\n", output));
    }
    if let Some(verdict) = task.audit_verdict.as_deref() {
        out.push_str("Audit: ");
        out.push_str(verdict);
        if let Some(report_path) = task.audit_report_path.as_deref() {
            out.push_str(&format!(" (report: {report_path})"));
        }
        out.push('\n');
    }
}

fn append_task_events(out: &mut String, events: &[TaskEvent]) {
    if events.is_empty() {
        return;
    }
    out.push_str("\nEvents:\n");
    for ev in events {
        let time = ev.timestamp.format("%H:%M:%S");
        let detail_lines = event_detail_lines(ev);
        out.push_str(&format!(
            "  {}  [{:>10}] {}\n",
            time,
            ev.event_kind.as_str(),
            detail_lines[0],
        ));
        for line in &detail_lines[1..] {
            out.push_str(&format!("                         {line}\n"));
        }
    }
}

fn event_detail_lines(event: &TaskEvent) -> Vec<String> {
    let mut lines = vec![truncate(&event.detail, 60)];
    if let Some(eval_output) = iterate_eval_output(event) {
        lines.push(format!("Eval output: {}", truncate(eval_output, 60)));
    }
    lines
}

fn iterate_eval_output(event: &TaskEvent) -> Option<&str> {
    event
        .metadata
        .as_ref()?
        .get("iterate")?
        .get("eval_output")?
        .as_str()
        .map(str::trim)
        .filter(|output| !output.is_empty() && *output != "(no output)")
}

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
