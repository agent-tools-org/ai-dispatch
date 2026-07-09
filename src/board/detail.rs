// Detailed task board rendering for audit-style views.
// Exports render_task_detail for crate::board.
// Deps: parent board formatting helpers and task/event types.

use crate::cost;
use crate::session;
use crate::types::{Task, TaskEvent, TaskStatus};
use std::path::Path;
use std::process::Command;

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
    append_delivery(out, task);
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

fn append_delivery(out: &mut String, task: &Task) {
    if task.final_head_sha.is_none() && task.final_branch.is_none() {
        return;
    };
    if let Some(final_sha) = task.final_head_sha.as_deref() {
        let repo_path = delivery_repo_path(task);
        let stat = match (repo_path, task.start_sha.as_deref()) {
            (Some(repo_path), Some(start_sha)) => delivery_diff_stat(repo_path, start_sha, final_sha),
            _ => DeliveryDiffStat::default(),
        };
        let short_sha = short_sha(final_sha);
        let subject = repo_path.and_then(|path| commit_subject(path, final_sha)).unwrap_or_default();
        out.push_str(&format!(
            "Delivered: {} files (+{}/-{}) — {} \"{}\"\n",
            stat.files, stat.insertions, stat.deletions, short_sha, subject,
        ));
    }
    if let Some(final_branch) = task.final_branch.as_deref() {
        out.push_str(&format!("Branch:    {final_branch}\n"));
        if task.worktree_branch.as_deref().is_some_and(|branch| branch != final_branch) {
            let original = task.worktree_branch.as_deref().unwrap_or("-");
            out.push_str(&format!(
                "  ⚠ agent switched branch: {original} -> {final_branch}\n"
            ));
        }
    }
}

fn delivery_repo_path(task: &Task) -> Option<&str> {
    task.repo_path
        .as_deref()
        .or_else(|| task.worktree_path.as_deref().filter(|path| Path::new(path).exists()))
}

#[derive(Default)]
struct DeliveryDiffStat {
    files: usize,
    insertions: usize,
    deletions: usize,
}

fn delivery_diff_stat(repo_path: &str, start_sha: &str, final_sha: &str) -> DeliveryDiffStat {
    let output = Command::new("git")
        .args(["-C", repo_path, "diff", "--numstat", &format!("{start_sha}..{final_sha}")])
        .output();
    let Ok(output) = output else {
        return DeliveryDiffStat::default();
    };
    if !output.status.success() {
        return DeliveryDiffStat::default();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_numstat_line)
        .fold(DeliveryDiffStat::default(), |mut stat, (ins, del)| {
            stat.files += 1;
            stat.insertions += ins;
            stat.deletions += del;
            stat
        })
}

fn parse_numstat_line(line: &str) -> Option<(usize, usize)> {
    let mut parts = line.split('\t');
    let insertions = parts.next()?.parse().ok()?;
    let deletions = parts.next()?.parse().ok()?;
    Some((insertions, deletions))
}

fn commit_subject(repo_path: &str, sha: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["-C", repo_path, "log", "-1", "--format=%s", sha])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn short_sha(sha: &str) -> &str {
    sha.get(..12).unwrap_or(sha)
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
