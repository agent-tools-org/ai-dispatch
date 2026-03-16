// Output and diff rendering for `aid show`.
// Exports: diff_text, output_text, output_text_for_task, log_text, read_task_output, read_tail.
// Deps: cmd::show::load_task, paths, Store, Task.
use anyhow::{Context, Result};
use serde_json::json;
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
        if task.worktree_branch.is_none() {
            out.push_str("\n[aid] In-place edit — use `git diff` to see working tree changes\n");
        }
        return Ok(out);
    }
    if task.worktree_branch.is_none() {
        // In-place task: show working tree diff from repo_path
        let repo = task.repo_path.as_deref().unwrap_or(".");
        let wt_diff = inplace_working_diff(repo);
        if !wt_diff.is_empty() {
            out.push_str("\n--- Working Tree Changes (in-place edit) ---\n");
            out.push_str(&wt_diff);
            return Ok(out);
        }
        out.push_str("\n--- Artifacts ---\n  (in-place edit — no uncommitted changes detected, may already be committed)\n");
    } else {
        out.push_str("\n--- Artifacts ---\n  (worktree removed or diff unavailable)\n");
    }
    Ok(out)
}

pub(crate) fn worktree_diff(task: &Task, task_id: &str) -> Result<String> {
    if let Some(ref worktree_path) = task.worktree_path
        && Path::new(worktree_path).exists()
    {
        return Ok(format_diff_output(worktree_path));
    }
    if let Some(fallback) = diff_artifact_fallback(task, task_id)? {
        return Ok(fallback);
    }
    Ok("\n--- Artifacts ---\n  (no worktree diff or output file available)\n".to_string())
}
fn format_diff_header(task: &Task) -> String {
    let mut out = String::new();
    out.push_str(&format!("=== Review: {} ===\n", task.id));
    out.push_str(&format!(
        "Agent: {}  Status: {}  Prompt: {}\n",
        task.agent_display_name(),
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
    // Try structured edit extraction from JSONL log before raw fallback
    let log_path = task
        .log_path
        .as_ref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| paths::log_path(task_id));
    if let Some(edits) = extract_edits_from_log(&log_path) {
        return Ok(Some(edits));
    }
    // Raw log fallback (last resort)
    if let Ok(log) = std::fs::read_to_string(&log_path) {
        return Ok(Some(format_artifact_block("Output", &log)));
    }
    Ok(None)
}

/// Parse JSONL log for tool_use edit/write events, return formatted diff.
fn extract_edits_from_log(log_path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(log_path).ok()?;
    let mut edits: Vec<(String, String, String, String)> = Vec::new(); // (file, tool, old, new)
    for line in content.lines() {
        let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some("tool_use") = obj.get("type").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(part) = obj.get("part") else { continue };
        let Some(tool) = part.get("tool").and_then(|v| v.as_str()) else {
            continue;
        };
        if !matches!(tool, "edit" | "write" | "apply_diff" | "replace") {
            continue;
        }
        let Some(input) = part.get("state").and_then(|s| s.get("input")) else {
            continue;
        };
        let file_path = input
            .get("filePath")
            .or_else(|| input.get("file_path"))
            .and_then(|v| v.as_str())
            .unwrap_or("(unknown)")
            .to_string();
        let old = input
            .get("oldString")
            .or_else(|| input.get("old_string"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let new = input
            .get("newString")
            .or_else(|| input.get("new_string"))
            .or_else(|| input.get("content"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        edits.push((file_path, tool.to_string(), old, new));
    }
    if edits.is_empty() {
        return None;
    }
    let mut out = String::new();
    out.push_str("\n--- File Changes (from agent log) ---\n");
    // File summary
    let mut files: Vec<&str> = edits.iter().map(|(f, ..)| f.as_str()).collect();
    files.dedup();
    out.push_str(&format!("  {} edit(s) across {} file(s):\n", edits.len(), files.len()));
    for f in &files {
        // Show short path (last 3 components)
        let short: String = f.rsplit('/').take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("/");
        out.push_str(&format!("    {short}\n"));
    }
    // Inline diffs
    for (file, tool, old, new) in &edits {
        let short: String = file.rsplit('/').take(2).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("/");
        out.push_str(&format!("\n  [{tool}] {short}\n"));
        if !old.is_empty() {
            for line in old.lines().take(10) {
                out.push_str(&format!("  - {line}\n"));
            }
            if old.lines().count() > 10 {
                out.push_str(&format!("  ... ({} more lines)\n", old.lines().count() - 10));
            }
        }
        if !new.is_empty() {
            for line in new.lines().take(10) {
                out.push_str(&format!("  + {line}\n"));
            }
            if new.lines().count() > 10 {
                out.push_str(&format!("  ... ({} more lines)\n", new.lines().count() - 10));
            }
        }
    }
    Some(out)
}
/// For in-place tasks: show `git diff` from the repo directory.
fn inplace_working_diff(repo_path: &str) -> String {
    let output = Command::new("git")
        .args(["-C", repo_path, "diff", "--", "."])
        .args(DIFF_EXCLUDE)
        .output()
        .ok();
    match output {
        Some(o) if o.status.success() && !o.stdout.is_empty() => {
            String::from_utf8_lossy(&o.stdout).into()
        }
        _ => String::new(),
    }
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

pub(crate) fn parse_diff_stat(diff_text: &str) -> Vec<serde_json::Value> {
    diff_text
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || !line.contains('|') {
                return None;
            }
            let mut parts = line.splitn(2, '|');
            let file = parts.next()?.trim();
            let stats = parts.next()?.trim();
            if stats.starts_with("Bin") {
                return None;
            }
            let insertions = stats.chars().filter(|c| *c == '+').count() as u64;
            let deletions = stats.chars().filter(|c| *c == '-').count() as u64;
            if insertions == 0 && deletions == 0 {
                return None;
            }
            Some(json!({
                "file": file,
                "insertions": insertions,
                "deletions": deletions,
            }))
        })
        .collect()
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
        if let Some(output) = run_git_diff(wt_path, &diff_args(args))
            && !output.trim().is_empty()
        {
            return output;
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
    use crate::types::{AgentKind, TaskId, TaskStatus, VerifyStatus};
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
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
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
    fn parse_diff_stat_standard_line() {
        let entries = parse_diff_stat(" src/foo.rs | 8 +++++---\n");
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry["file"], json!("src/foo.rs"));
        assert_eq!(entry["insertions"], json!(5));
        assert_eq!(entry["deletions"], json!(3));
    }
    #[test]
    fn parse_diff_stat_skips_binary_entries() {
        assert!(parse_diff_stat(" src/bin.dat | Bin 0 -> 123 bytes\n").is_empty());
    }
    #[test]
    fn parse_diff_stat_empty_text() {
        assert!(parse_diff_stat("").is_empty());
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
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            read_only: false,
            budget: false,
        };
        store.insert_task(&task).unwrap();

        let text = diff_text(&store, "t-log-fallback").unwrap();

        assert!(text.contains("\n--- Output ---\nlog output\n"));
        assert!(!text.contains("no worktree diff or output file available"));
    }
}
