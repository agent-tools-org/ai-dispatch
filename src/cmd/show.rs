// Handler for `aid show <task-id>` — unified task inspection.
// Combines events, diff, output, log, and AI explanation into one command.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use crate::board::render_task_detail;
use crate::cmd;
use crate::paths;
use crate::store::Store;
use crate::types::{EventKind, Task, TaskStatus};

pub struct ShowArgs {
    pub task_id: String,
    pub context: bool,
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
    Context,
    Diff,
    Output,
    Log,
}

pub async fn run(store: Arc<Store>, args: ShowArgs) -> Result<()> {
    if args.context {
        let text = render_mode_text(&store, &args.task_id, ShowMode::Context)?;
        print!("{text}");
        return Ok(());
    }
    if args.explain {
        return cmd::explain::run_explain(store, &args.task_id, args.agent, args.model).await;
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
        ShowMode::Context => context_text(store, task_id),
        ShowMode::Diff => diff_text(store, task_id),
        ShowMode::Output => output_text(store, task_id),
        ShowMode::Log => log_text(task_id),
    }
}

// --- Default mode: events + stderr + diff stat ---

pub fn audit_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = load_task(store, task_id)?;
    let events = store.get_events(task_id)?;
    let retry_chain = if task.parent_task_id.is_some() {
        Some(store.get_retry_chain(task_id)?)
    } else {
        None
    };
    let mut out = render_task_detail(&task, &events, retry_chain);

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

pub fn context_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = load_task(store, task_id)?;
    let mut out = String::new();
    out.push_str("=== Original Prompt ===\n");
    out.push_str(&task.prompt);
    out.push_str("\n");

    if let Some(resolved_prompt) = task.resolved_prompt.as_deref() {
        out.push_str("\n=== Resolved Prompt ===\n");
        out.push_str(resolved_prompt);
        out.push_str("\n");
        return Ok(out);
    }

    let (skill_content, resolved_prompt) = reconstruct_context(store, &task)?;
    out.push_str("\n(reconstructed — context files may have changed since dispatch)\n");
    out.push_str("\n=== Injected Skills ===\n");
    if skill_content.trim().is_empty() {
        out.push_str("(none)\n");
    } else {
        out.push_str(&skill_content);
        out.push_str("\n");
    }
    out.push_str("\n=== Resolved Prompt ===\n");
    out.push_str(&resolved_prompt);
    out.push_str("\n");
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

    if task.output_path.is_none() {
        let log_path = paths::log_path(task_id);
        if let Ok(log) = std::fs::read_to_string(&log_path) {
            out.push_str("\n--- Output ---\n");
            out.push_str(&log);
            return Ok(out);
        }
    }

    out.push_str("\n--- Artifacts ---\n  (no worktree diff or output file available)\n");
    Ok(out)
}

// --- Output mode ---

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
    let task = load_task(store, task_id)?;
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

// --- Log mode ---

pub fn log_text(task_id: &str) -> Result<String> {
    let path = paths::log_path(task_id);
    std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read log file {}", path.display()))
}

// --- Shared helpers ---

pub(crate) fn load_task(store: &Arc<Store>, task_id: &str) -> Result<Task> {
    store
        .get_task(task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task '{task_id}' not found"))
}

fn reconstruct_context(store: &Arc<Store>, task: &Task) -> Result<(String, String)> {
    let workgroup = if let Some(group_id) = task.workgroup_id.as_deref() {
        store.get_workgroup(group_id)?
    } else {
        None
    };
    let milestones = if let Some(group_id) = task.workgroup_id.as_deref() {
        store.get_workgroup_milestones(group_id)?
    } else {
        vec![]
    };
    let skill_names = crate::skills::auto_skills(&task.agent, task.worktree_path.is_some());
    let skill_parts = skill_names
        .iter()
        .map(|skill| {
            crate::skills::resolve_skill_content(skill)
                .unwrap_or_else(|err| format!("[missing skill: {skill}: {err}]"))
        })
        .collect::<Vec<_>>();
    let skill_content = skill_parts.join("\n\n");
    let mut resolved_prompt = crate::workgroup::compose_prompt(
        &task.prompt,
        None,
        workgroup.as_ref(),
        &milestones,
    );
    if !skill_content.is_empty() {
        resolved_prompt = format!("{resolved_prompt}\n\n--- Methodology ---\n{skill_content}");
    }
    resolved_prompt = crate::templates::inject_milestone_prompt(&resolved_prompt);
    Ok((skill_content, resolved_prompt))
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
    if let Some(s) = git_output(wt_path, &["diff", "main...HEAD", "--stat"]) {
        if !s.trim().is_empty() {
            return s;
        }
    }
    if let Some(s) = git_output(wt_path, &["diff", "--stat"]) {
        if !s.trim().is_empty() {
            return s;
        }
    }
    git_output(wt_path, &["diff", "--stat", "HEAD~1"])
        .unwrap_or_else(|| "  (no changes detected)\n".to_string())
}

fn full_diff(wt_path: &str) -> String {
    if let Some(s) = git_output(wt_path, &["diff", "main...HEAD"]) {
        if !s.trim().is_empty() {
            return s;
        }
    }
    if let Some(s) = git_output(wt_path, &["diff"]) {
        if !s.trim().is_empty() {
            return s;
        }
    }
    git_output(wt_path, &["diff", "HEAD~1"])
        .unwrap_or_else(|| "  (no diff available)\n".to_string())
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
    use crate::types::{AgentKind, TaskId};
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
        };
        assert_eq!(read_task_output(&task).unwrap(), "hello\n");
    }

    #[test]
    fn tail_lines_keeps_only_requested_suffix() {
        assert_eq!(tail_lines("a\nb\nc\nd", 2), "c\nd");
    }

    #[test]
    fn context_text_prefers_stored_resolved_prompt() {
        let store = Arc::new(Store::open_memory().unwrap());
        let task = Task {
            id: TaskId("t-context".to_string()),
            agent: AgentKind::Codex,
            prompt: "raw prompt".to_string(),
            resolved_prompt: Some("resolved prompt".to_string()),
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
        };
        store.insert_task(&task).unwrap();

        let text = context_text(&store, "t-context").unwrap();

        assert!(text.contains("=== Original Prompt ===\nraw prompt"));
        assert!(text.contains("=== Resolved Prompt ===\nresolved prompt"));
        assert!(!text.contains("(reconstructed"));
    }

    #[test]
    fn context_text_reconstructs_skills_when_resolved_prompt_missing() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
        let dir = crate::paths::aid_dir().join("skills");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("implementer.md"), "# Implementer").unwrap();

        let store = Arc::new(Store::open_memory().unwrap());
        let task = Task {
            id: TaskId("t-reconstruct".to_string()),
            agent: AgentKind::Codex,
            prompt: "raw prompt".to_string(),
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
        };
        store.insert_task(&task).unwrap();

        let text = context_text(&store, "t-reconstruct").unwrap();

        assert!(text.contains("(reconstructed"));
        assert!(text.contains("=== Injected Skills ===\n# Implementer"));
        assert!(text.contains("=== Resolved Prompt ===\nraw prompt"));
        assert!(text.contains("[MILESTONE] <brief description>"));
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
        };
        store.insert_task(&task).unwrap();

        let text = diff_text(&store, "t-log-fallback").unwrap();

        assert!(text.contains("\n--- Output ---\nlog output\n"));
        assert!(!text.contains("no worktree diff or output file available"));
    }
}
