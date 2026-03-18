// Output and diff rendering for `aid show`.
// Exports: diff_text, output_text, output_text_for_task, log_text, read_task_output, read_tail.
// Deps: cmd::show::load_task, paths, Store, Task.
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
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
pub fn output_text_for_task(store: &Store, task_id: &str, full: bool) -> Result<String> {
    let task = load_task_for_output(task_id, store)?;
    if let Ok(content) = read_task_output(&task) {
        return Ok(content);
    }
    if let Some(content) = extract_messages_for_task(&task, task_id, full) {
        return Ok(content);
    }
    let tail_lines = if full { 200 } else { 50 };
    let path = task_log_path(&task, task_id);
    Ok(read_tail(&path, tail_lines, "No output or log available"))
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
    if let Some(content) = extract_messages_for_task(&task, task_id, false) {
        return Ok(content);
    }
    let path = task_log_path(&task, task_id);
    Ok(read_tail(&path, 50, "No output or log available"))
}

pub fn output_text_full(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = super::load_task(store, task_id)?;
    if let Ok(content) = read_task_output(&task) {
        return Ok(content);
    }
    if let Some(content) = extract_messages_for_task(&task, task_id, true) {
        return Ok(content);
    }
    let path = task_log_path(&task, task_id);
    Ok(read_tail(&path, 200, "No output or log available"))
}

fn task_log_path(task: &Task, task_id: &str) -> PathBuf {
    task.log_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| paths::log_path(task_id))
}

fn extract_messages_for_task(task: &Task, task_id: &str, full: bool) -> Option<String> {
    extract_messages_from_log(&task_log_path(task, task_id), full)
}

pub(crate) fn extract_messages_from_log(log_path: &Path, full: bool) -> Option<String> {
    const MAX_MESSAGE_CHARS: usize = 1_000;
    const MAX_OUTPUT_CHARS: usize = 8_000;
    const HEAD_MESSAGE_COUNT: usize = 3;
    const TAIL_MESSAGE_COUNT: usize = 7;

    let content = std::fs::read_to_string(log_path).ok()?;
    let mut messages = Vec::new();
    let mut streaming_message = String::new();
    for line in content.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match value.get("type").and_then(|kind| kind.as_str()) {
            Some("item.completed") => {
                let Some(item) = value.get("item") else { continue };
                let is_agent_message =
                    item.get("type").and_then(|kind| kind.as_str()) == Some("agent_message");
                let Some(text) = item.get("text").and_then(|text| text.as_str()) else {
                    continue;
                };
                if is_agent_message {
                    messages.push(text.to_string());
                }
            }
            Some("message") => {
                let is_assistant =
                    value.get("role").and_then(|role| role.as_str()) == Some("assistant");
                let Some(content) = value.get("content").and_then(|text| text.as_str()) else {
                    continue;
                };
                if !is_assistant {
                    continue;
                }
                if value.get("delta").and_then(|delta| delta.as_bool()) == Some(true) {
                    streaming_message.push_str(content);
                } else {
                    if !streaming_message.is_empty() {
                        messages.push(std::mem::take(&mut streaming_message));
                    }
                    messages.push(content.to_string());
                }
            }
            Some("text") => {
                let Some(text) = value
                    .get("content")
                    .and_then(|text| text.as_str())
                    .or_else(|| value.get("text").and_then(|text| text.as_str()))
                    .or_else(|| value.pointer("/part/text").and_then(|text| text.as_str()))
                else {
                    continue;
                };
                if !streaming_message.is_empty() {
                    messages.push(std::mem::take(&mut streaming_message));
                }
                messages.push(text.to_string());
            }
            _ => {}
        }
    }
    if !streaming_message.is_empty() {
        messages.push(streaming_message);
    }

    if messages.is_empty() {
        return None;
    }

    if !full {
        for message in &mut messages {
            if message.len() > MAX_MESSAGE_CHARS {
                message.truncate(message.floor_char_boundary(MAX_MESSAGE_CHARS.saturating_sub(3)));
                message.push_str("...");
            }
        }

        if messages.len() > HEAD_MESSAGE_COUNT + TAIL_MESSAGE_COUNT {
            let omitted = messages.len() - HEAD_MESSAGE_COUNT - TAIL_MESSAGE_COUNT;
            let mut capped =
                Vec::with_capacity(HEAD_MESSAGE_COUNT + TAIL_MESSAGE_COUNT + 1);
            capped.extend(messages[..HEAD_MESSAGE_COUNT].iter().cloned());
            capped.push(format!("[... {omitted} messages omitted ...]"));
            capped.extend(messages[messages.len() - TAIL_MESSAGE_COUNT..].iter().cloned());
            messages = capped;
        }
    }

    let mut output = messages.join("\n---\n");
    if !full && output.len() > MAX_OUTPUT_CHARS {
        output.truncate(output.floor_char_boundary(MAX_OUTPUT_CHARS.saturating_sub(3)));
        output.push_str("...");
    }

    Some(output)
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
    use serde_json::json;
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
    #[test]
    fn extract_messages_from_log_collects_supported_formats() {
        let file = NamedTempFile::new().unwrap();
        let content = [
            json!({
                "type": "item.completed",
                "item": { "type": "agent_message", "text": "codex message" }
            }),
            json!({
                "type": "message",
                "role": "assistant",
                "content": "stream ",
                "delta": true
            }),
            json!({
                "type": "message",
                "role": "assistant",
                "content": "delta",
                "delta": true
            }),
            json!({
                "type": "text",
                "part": { "text": "opencode text part" }
            }),
            json!({
                "type": "text",
                "content": "gemini text event"
            }),
        ]
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
        std::fs::write(file.path(), content).unwrap();

        let output = extract_messages_from_log(file.path(), false);

        assert_eq!(
            output,
            Some(
                "codex message\n---\nstream delta\n---\nopencode text part\n---\ngemini text event"
                    .to_string()
            )
        );
    }
    #[test]
    fn extract_messages_from_log_returns_none_without_supported_messages() {
        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "{\"type\":\"event\"}\nnot-json\n").unwrap();

        assert_eq!(extract_messages_from_log(file.path(), false), None);
    }
    #[test]
    fn extract_messages_from_log_caps_message_count_and_size() {
        let file = NamedTempFile::new().unwrap();
        let content = (0..22)
            .map(|index| {
                serde_json::to_string(&json!({
                    "type": "message",
                    "role": "assistant",
                    "content": format!("message-{index:02}-{}", "x".repeat(500)),
                }))
            })
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join("\n");
        std::fs::write(file.path(), content).unwrap();

        let output = extract_messages_from_log(file.path(), false).unwrap();
        let parts = output.split("\n---\n").collect::<Vec<_>>();

        assert_eq!(output.matches("\n---\n").count(), 10);
        assert_eq!(parts.len(), 11);
        assert!(parts[3].starts_with("[... 12 messages omitted ...]"));
        assert!(parts[0].starts_with("message-00-"));
        assert!(parts[10].starts_with("message-21-"));
        assert!(parts.iter().all(|part| part.len() <= 1_000));
        assert!(output.len() <= 8_000);
    }
    #[test]
    fn extract_messages_full_skips_truncation() {
        let file = NamedTempFile::new().unwrap();
        let content = (0..22)
            .map(|index| {
                serde_json::to_string(&json!({
                    "type": "message",
                    "role": "assistant",
                    "content": format!("message-{index:02}-{}", "x".repeat(500)),
                }))
            })
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join("\n");
        std::fs::write(file.path(), content).unwrap();

        let output = extract_messages_from_log(file.path(), true).unwrap();
        let parts: Vec<&str> = output.split("\n---\n").collect();

        // Full mode: all 22 messages, no omissions, no per-message truncation
        assert_eq!(parts.len(), 22);
        assert!(parts[0].starts_with("message-00-"));
        assert!(parts[21].starts_with("message-21-"));
        assert!(!output.contains("[... "));
        // Each message is 512 chars ("message-XX-" + 500 x's), not truncated
        assert!(parts.iter().all(|part| part.len() > 500));
    }
    #[test]
    fn output_text_for_task_prefers_extracted_messages_to_raw_log() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
        std::fs::create_dir_all(crate::paths::logs_dir()).unwrap();
        let log_path = crate::paths::log_path("t-output-messages");
        let log_content = [
            json!({
                "type": "message",
                "role": "assistant",
                "content": "human-readable output"
            }),
            json!({
                "type": "text",
                "part": { "text": "second chunk" }
            }),
        ]
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
        std::fs::write(&log_path, log_content).unwrap();

        let store = Store::open_memory().unwrap();
        let task = Task {
            id: TaskId("t-output-messages".to_string()),
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

        let output = output_text_for_task(&store, "t-output-messages", false).unwrap();

        assert_eq!(output, "human-readable output\n---\nsecond chunk");
    }
}
