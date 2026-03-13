// Handler for `aid show <task-id>` — unified task inspection.
// Combines events, diff, output, log, and AI explanation into one command.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use crate::board::render_task_detail;
use crate::cmd::run::{self, RunArgs};
use crate::paths;
use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent, TaskStatus};

pub struct ShowArgs {
    pub task_id: String,
    pub diff: bool,
    pub output: bool,
    pub explain: bool,
    pub log: bool,
    pub agent: Option<String>,
    pub model: Option<String>,
}

#[derive(Clone, Copy)]
pub enum ShowMode {
    Summary,
    Diff,
    Output,
    Log,
}

pub async fn run(store: Arc<Store>, args: ShowArgs) -> Result<()> {
    if args.explain {
        return run_explain(store, &args.task_id, args.agent, args.model).await;
    }
    let mode = if args.diff {
        ShowMode::Diff
    } else if args.output {
        ShowMode::Output
    } else if args.log {
        ShowMode::Log
    } else {
        ShowMode::Summary
    };
    let text = render_mode_text(&store, &args.task_id, mode)?;
    print!("{text}");
    Ok(())
}

pub fn render_mode_text(store: &Arc<Store>, task_id: &str, mode: ShowMode) -> Result<String> {
    match mode {
        ShowMode::Summary => audit_text(store, task_id),
        ShowMode::Diff => diff_text(store, task_id),
        ShowMode::Output => output_text(store, task_id),
        ShowMode::Log => log_text(task_id),
    }
}

// --- Default mode: events + stderr + diff stat ---

pub fn audit_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = load_task(store, task_id)?;
    let events = store.get_events(task_id)?;
    let mut out = render_task_detail(&task, &events);

    if task.status == TaskStatus::Failed
        && let Some(stderr) = stderr_tail(task_id)
    {
        out.push_str("\nStderr:\n");
        out.push_str(&stderr);
    }

    if let Some(ref wt_path) = task.worktree_path
        && Path::new(wt_path).exists()
    {
        out.push_str("\nChanges:\n");
        out.push_str(&diff_stat(wt_path));
    }

    Ok(out)
}

// --- Diff mode: full worktree diff ---

pub fn diff_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = load_task(store, task_id)?;
    let mut out = String::new();
    out.push_str(&format!("=== Review: {} ===\n", task.id));
    out.push_str(&format!(
        "Agent: {}  Status: {}  Prompt: {}\n",
        task.agent,
        task.status.label(),
        truncate(&task.prompt, 60),
    ));
    if let Some(ref model) = task.model {
        out.push_str(&format!("Model: {model}\n"));
    }

    let events = store.get_events(task_id)?;
    if !events.is_empty() {
        out.push_str("\n--- Events (last 10) ---\n");
        let start = events.len().saturating_sub(10);
        for event in &events[start..] {
            let kind = event.event_kind.as_str();
            let time = event.timestamp.format("%H:%M:%S");
            let detail = truncate(&event.detail, 80);
            let marker = if event.event_kind == EventKind::Error {
                "!"
            } else {
                " "
            };
            out.push_str(&format!("{marker} [{time}] {kind}: {detail}\n"));
        }
    }

    if let Some(ref worktree_path) = task.worktree_path
        && Path::new(worktree_path).exists()
    {
        out.push_str("\n--- Diff Stat ---\n");
        out.push_str(&diff_stat(worktree_path));
        out.push_str("\n--- Full Diff ---\n");
        out.push_str(&full_diff(worktree_path));
        out.push_str(&format!("\nWorktree: {worktree_path}\n"));
        return Ok(out);
    }

    if let Ok(task_output) = read_task_output(&task) {
        out.push_str("\n--- Output ---\n");
        out.push_str(&task_output);
        return Ok(out);
    }

    if let Some(ref log_path) = task.log_path
        && let Ok(log) = std::fs::read_to_string(log_path)
    {
        out.push_str("\n--- Log ---\n");
        out.push_str(&log);
        return Ok(out);
    }

    out.push_str("\n--- Artifacts ---\n  (no worktree diff or output file available)\n");
    Ok(out)
}

// --- Output mode ---

pub fn output_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = load_task(store, task_id)?;
    read_task_output(&task)
}

pub fn read_task_output(task: &Task) -> Result<String> {
    let path = task
        .output_path
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Task has no output file"))?;
    std::fs::read_to_string(path).with_context(|| format!("Failed to read output file {path}"))
}

// --- Log mode ---

pub fn log_text(task_id: &str) -> Result<String> {
    let path = paths::log_path(task_id);
    std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read log file {}", path.display()))
}

// --- Explain mode ---

async fn run_explain(
    store: Arc<Store>,
    task_id: &str,
    agent: Option<String>,
    model: Option<String>,
) -> Result<()> {
    let task = load_task(&store, task_id)?;
    let events = store.get_events(task_id)?;
    let stderr = read_tail(&paths::stderr_path(task_id), 30, "stderr unavailable");
    let log = read_tail(&paths::log_path(task_id), 50, "log unavailable");
    let context = build_explain_context(&task, &events, &stderr, &log);
    let prompt = build_explain_prompt(&context);
    let agent_name = agent.unwrap_or_else(|| "gemini".to_string());

    println!("[explain] Analyzing task {task_id} via {agent_name}...");
    let _ = run::run(
        store,
        RunArgs {
            agent_name,
            prompt,
            dir: None,
            output: None,
            model,
            worktree: None,
            group: None,
            verify: None,
            retry: 0,
            context: vec![],
            skills: vec![],
            background: false,
            announce: true,
            parent_task_id: Some(task_id.to_string()),
            on_done: None,
        },
    )
    .await?;
    Ok(())
}

fn build_explain_context(
    task: &Task,
    events: &[TaskEvent],
    stderr_tail: &str,
    log_tail: &str,
) -> String {
    format!(
        "[Task Info]\n{}\n\n[Events Timeline]\n{}\n\n[Stderr Tail]\n{}\n\n[Log Tail]\n{}",
        format_task_info(task),
        format_events(events),
        stderr_tail,
        log_tail,
    )
}

fn build_explain_prompt(context: &str) -> String {
    format!(
        concat!(
            "You are explaining a prior `aid` task execution.\n",
            "Analyze the context and answer with these sections:\n",
            "1. Summary: exactly one sentence.\n",
            "2. Intent: what the task tried to do.\n",
            "3. What Happened: concise timeline of execution.\n",
            "4. Root Cause: likely reason for failure, or say that no failure is evident.\n",
            "Be concrete, use the evidence in the artifacts, and say when evidence is missing.\n\n",
            "[Execution Context]\n",
            "{}"
        ),
        context,
    )
}

fn format_task_info(task: &Task) -> String {
    let completed = task
        .completed_at
        .map(|v| v.to_rfc3339())
        .unwrap_or_else(|| "(not completed)".to_string());
    let duration = task
        .duration_ms
        .map(|v| format!("{v} ms"))
        .unwrap_or_else(|| "(unknown)".to_string());
    let tokens = task
        .tokens
        .map(|v| v.to_string())
        .unwrap_or_else(|| "(unknown)".to_string());
    let cost = task
        .cost_usd
        .map(|v| format!("{v:.4}"))
        .unwrap_or_else(|| "(unknown)".to_string());

    [
        format!("Task ID: {}", task.id),
        format!("Agent: {}", task.agent),
        format!("Status: {}", task.status.label()),
        format!("Prompt: {}", task.prompt),
        format!(
            "Parent Task ID: {}",
            task.parent_task_id.as_deref().unwrap_or("(none)")
        ),
        format!("Model: {}", task.model.as_deref().unwrap_or("(none)")),
        format!("Created At: {}", task.created_at.to_rfc3339()),
        format!("Completed At: {completed}"),
        format!("Duration: {duration}"),
        format!("Tokens: {tokens}"),
        format!("Cost USD: {cost}"),
        format!(
            "Worktree: {}",
            task.worktree_path.as_deref().unwrap_or("(none)")
        ),
        format!(
            "Output Path: {}",
            task.output_path.as_deref().unwrap_or("(none)")
        ),
        format!(
            "Stderr Path: {}",
            paths::stderr_path(task.id.as_str()).display()
        ),
        format!("Log Path: {}", paths::log_path(task.id.as_str()).display()),
    ]
    .join("\n")
}

fn format_events(events: &[TaskEvent]) -> String {
    if events.is_empty() {
        return "(no events recorded)".to_string();
    }
    events
        .iter()
        .map(|e| {
            let metadata = e
                .metadata
                .as_ref()
                .map(|v| format!(" metadata={v}"))
                .unwrap_or_default();
            format!(
                "{} {} {}{}",
                e.timestamp.to_rfc3339(),
                e.event_kind.as_str(),
                e.detail,
                metadata,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// --- Shared helpers ---

fn load_task(store: &Arc<Store>, task_id: &str) -> Result<Task> {
    store
        .get_task(task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task '{task_id}' not found"))
}

fn stderr_tail(task_id: &str) -> Option<String> {
    let content = std::fs::read_to_string(paths::stderr_path(task_id)).ok()?;
    if content.is_empty() {
        return None;
    }
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(20);
    let mut out = String::new();
    if start > 0 {
        out.push_str(&format!("  ... ({start} lines omitted)\n"));
    }
    for line in &lines[start..] {
        out.push_str(&format!("  {line}\n"));
    }
    Some(out)
}

fn diff_stat(wt_path: &str) -> String {
    match git_output(wt_path, &["diff", "--stat"]) {
        Some(s) if !s.trim().is_empty() => s,
        Some(_) => git_output(wt_path, &["diff", "--stat", "HEAD~1"])
            .unwrap_or_else(|| "  (no changes detected)\n".to_string()),
        None => "  (could not read git diff)\n".to_string(),
    }
}

fn full_diff(wt_path: &str) -> String {
    match git_output(wt_path, &["diff"]) {
        Some(s) if !s.trim().is_empty() => s,
        Some(_) => git_output(wt_path, &["diff", "HEAD~1"])
            .unwrap_or_else(|| "  (no diff available)\n".to_string()),
        None => "  (could not read git diff)\n".to_string(),
    }
}

fn git_output(wt_path: &str, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(wt_path)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into())
}

fn read_tail(path: &Path, limit: usize, unavailable: &str) -> String {
    let Ok(bytes) = std::fs::read(path) else {
        return unavailable.to_string();
    };
    let content = String::from_utf8_lossy(&bytes);
    let tail = tail_lines(&content, limit);
    if tail.is_empty() {
        unavailable.to_string()
    } else {
        tail
    }
}

fn tail_lines(content: &str, limit: usize) -> String {
    content
        .lines()
        .rev()
        .take(limit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AgentKind, TaskId};
    use chrono::Local;
    use tempfile::NamedTempFile;

    #[test]
    fn reads_task_output_file() {
        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "hello\n").unwrap();
        let task = Task {
            id: TaskId("t-output".to_string()),
            agent: AgentKind::Gemini,
            prompt: "prompt".to_string(),
            status: TaskStatus::Done,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            worktree_path: None,
            worktree_branch: None,
            log_path: None,
            output_path: Some(file.path().display().to_string()),
            tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            created_at: Local::now(),
            completed_at: None,
        };
        assert_eq!(read_task_output(&task).unwrap(), "hello\n");
    }

    #[test]
    fn tail_lines_keeps_only_requested_suffix() {
        assert_eq!(tail_lines("a\nb\nc\nd", 2), "c\nd");
    }

    #[test]
    fn explain_prompt_embeds_execution_context() {
        let prompt = build_explain_prompt("task context");
        assert!(prompt.contains("Summary: exactly one sentence."));
        assert!(prompt.contains("[Execution Context]\ntask context"));
    }
}
