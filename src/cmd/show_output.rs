// Output and diff rendering for `aid show`.
// Exports: diff_text, output_text, output_text_for_task, log_text, read_task_output, read_tail.
// Deps: cmd::show::load_task, paths, Store, Task.
use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use crate::paths;
use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent};
const DIFF_EXCLUDE: &[&str] = &[":(exclude)*.lock", ":(exclude)package-lock.json"];
pub fn diff_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = super::load_task(store, task_id)?;
    let mut out = format_diff_header(&task);
    let events = store.get_events(task_id)?;
    if !events.is_empty() {
        out.push_str(&format_recent_events(&events));
    }
    if let Some(ref worktree_path) = task.worktree_path
        && Path::new(worktree_path).exists()
    {
        out.push_str(&format_diff_output(worktree_path));
        out.push_str(&format!("\nWorktree: {worktree_path}\n"));
        return Ok(out);
    }
    if let Some(fallback) = diff_artifact_fallback(&task, task_id)? {
        out.push_str(&fallback);
        return Ok(out);
    }
    out.push_str("\n--- Artifacts ---\n  (no worktree diff or output file available)\n");
    Ok(out)
}
fn format_diff_header(task: &Task) -> String {
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
    out
}
fn format_recent_events(events: &[TaskEvent]) -> String {
    let mut out = String::new();
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
    out
}
fn format_diff_output(worktree_path: &str) -> String {
    let mut out = String::new();
    out.push_str("\n--- Diff Stat ---\n");
    out.push_str(&diff_stat(worktree_path));
    out.push_str("\n--- Full Diff ---\n");
    out.push_str(&full_diff(worktree_path));
    out
}
fn diff_artifact_fallback(task: &Task, task_id: &str) -> Result<Option<String>> {
    if let Ok(task_output) = read_task_output(task) {
        return Ok(Some(format_artifact_block("Output", &task_output)));
    }
    if let Some(ref log_path) = task.log_path
        && let Ok(log) = std::fs::read_to_string(log_path)
    {
        return Ok(Some(format_artifact_block("Log", &log)));
    }
    if task.output_path.is_none() {
        let log_path = paths::log_path(task_id);
        if let Ok(log) = std::fs::read_to_string(&log_path) {
            return Ok(Some(format_artifact_block("Output", &log)));
        }
    }
    Ok(None)
}
fn format_artifact_block(title: &str, content: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("\n--- {title} ---\n"));
    out.push_str(content);
    out
}
pub fn output_text_for_task(store: &Store, task_id: &str) -> Result<String> {
    let task = load_task_for_output(task_id, store)?;
    if let Ok(content) = read_task_output(&task) {
        return Ok(content);
    }
    if let Some(ref log_path) = task.log_path {
        let path = Path::new(log_path);
        return Ok(read_tail(path, 50, "No output or log available"));
    }
    let path = paths::log_path(task_id);
    Ok(read_tail(&path, 50, "No output or log available"))
}
fn load_task_for_output(task_id: &str, store: &Store) -> Result<Task> {
    store
        .get_task(task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task '{task_id}' not found"))
}
pub fn output_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = super::load_task(store, task_id)?;
    if let Ok(content) = read_task_output(&task) {
        return Ok(content);
    }
    if let Some(ref log_path) = task.log_path {
        let path = Path::new(log_path);
        return Ok(read_tail(path, 50, "No output or log available"));
    }
    let path = paths::log_path(task_id);
    Ok(read_tail(&path, 50, "No output or log available"))
}
pub fn read_task_output(task: &Task) -> Result<String> {
    let path = task
        .output_path
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Task has no output file"))?;
    std::fs::read_to_string(path).with_context(|| format!("Failed to read output file {path}"))
}
pub fn log_text(task_id: &str) -> Result<String> {
    let path = paths::log_path(task_id);
    std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read log file {}", path.display()))
}
fn diff_args<'a>(base_args: &'a [&'a str]) -> Vec<&'a str> {
    let mut args = base_args.to_vec();
    args.extend_from_slice(&["--", "."]);
    args.extend_from_slice(DIFF_EXCLUDE);
    args
}
pub(crate) fn diff_stat(wt_path: &str) -> String {
    generate_diff(
        wt_path,
        &[
            &["diff", "main...HEAD", "--stat"],
            &["diff", "--stat"],
            &["diff", "--stat", "HEAD~1"],
        ],
        "  (no changes detected)\n",
    )
}
fn full_diff(wt_path: &str) -> String {
    generate_diff(
        wt_path,
        &[
            &["diff", "main...HEAD"],
            &["diff"],
            &["diff", "HEAD~1"],
        ],
        "  (no diff available)\n",
    )
}
fn generate_diff(wt_path: &str, args_sets: &[&[&str]], fallback: &str) -> String {
    for args in args_sets {
        if let Some(output) = run_git_diff(wt_path, &diff_args(args)) {
            if !output.trim().is_empty() {
                return output;
            }
        }
    }
    fallback.to_string()
}
fn run_git_diff(wt_path: &str, args: &[&str]) -> Option<String> {
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
pub(crate) fn read_tail(path: &Path, limit: usize, unavailable: &str) -> String {
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
        let end = s.floor_char_boundary(max.saturating_sub(3));
        format!("{}...", &s[..end])
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AgentKind, TaskId, TaskStatus};
    use chrono::Local;
    use std::sync::Arc;
    use tempfile::NamedTempFile;
    #[test]
    fn reads_task_output_file() {
        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "hello\n").unwrap();
        let task = Task {
            id: TaskId("t-output".to_string()),
            agent: AgentKind::Gemini,
            custom_agent_name: None,
            prompt: "prompt".to_string(),
            resolved_prompt: None,
            status: TaskStatus::Done,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            log_path: None,
            output_path: Some(file.path().display().to_string()),
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            read_only: false,
            budget: false,
        };
        assert_eq!(read_task_output(&task).unwrap(), "hello\n");
    }
    #[test]
    fn tail_lines_keeps_only_requested_suffix() {
        assert_eq!(tail_lines("a\nb\nc\nd", 2), "c\nd");
    }
    #[test]
    fn diff_text_falls_back_to_default_log_output() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
        std::fs::create_dir_all(crate::paths::logs_dir()).unwrap();
        std::fs::write(crate::paths::log_path("t-log-fallback"), "log output\n").unwrap();

        let store = Arc::new(Store::open_memory().unwrap());
        let task = Task {
            id: TaskId("t-log-fallback".to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "prompt".to_string(),
            resolved_prompt: None,
            status: TaskStatus::Done,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            read_only: false,
            budget: false,
        };
        store.insert_task(&task).unwrap();

        let text = diff_text(&store, "t-log-fallback").unwrap();

        assert!(text.contains("\n--- Output ---\nlog output\n"));
        assert!(!text.contains("no worktree diff or output file available"));
    }
}
