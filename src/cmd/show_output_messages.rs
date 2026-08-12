// Output and log rendering helpers for `aid show`.
// Exports: output_text, output_text_brief, log_text, log_text_brief, read_task_output, read_tail.
// Deps: paths, Store, Task, serde_json::Value.
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::paths;
use crate::store::Store;
use crate::types::{AgentKind, Task};
use super::show_output_extract::collect_messages;

pub fn output_text_for_task(store: &Store, task_id: &str, full: bool) -> Result<String> {
    let task = load_task_for_output(task_id, store)?;
    render_task_output(&task, task_id, full, 200)
}

fn load_task_for_output(task_id: &str, store: &Store) -> Result<Task> {
    store
        .get_task(task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task '{task_id}' not found"))
}

pub fn output_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = super::super::load_task(store, task_id)?;
    render_task_output(&task, task_id, true, 200)
}

pub fn output_text_brief(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = super::super::load_task(store, task_id)?;
    render_task_output(&task, task_id, false, 50)
}

fn render_task_output(task: &Task, task_id: &str, full: bool, tail_lines: usize) -> Result<String> {
    if let Ok(content) = read_task_output(task) {
        return Ok(content);
    }
    let absence = super::show_output_owned::missing_owned_output_notice(task);
    let body = if !full && is_research_task(task) {
        let path = task_log_path(task, task_id);
        extract_messages_research(&path)
    } else {
        None
    }
    .or_else(|| extract_messages_for_task(task, task_id, full))
    .unwrap_or_else(|| {
        let path = task_log_path(task, task_id);
        read_tail(&path, tail_lines, "No output or log available")
    });
    Ok(match absence {
        Some(notice) => format!("{notice}{body}"),
        None => body,
    })
}

fn task_log_path(task: &Task, task_id: &str) -> PathBuf {
    task.log_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| paths::log_path(task_id))
}

fn is_research_task(task: &Task) -> bool {
    task.worktree_path.is_none() && task.worktree_branch.is_none()
}

pub(crate) const UNRECOGNIZED_JSON_NOTICE_PREFIX: &str = "[Unrecognized JSON log format";

fn extract_messages_for_task(task: &Task, task_id: &str, full: bool) -> Option<String> {
    extract_messages_from_log(&task_log_path(task, task_id), full, Some(task.agent_display_name()))
}

pub(crate) fn extract_messages_from_log(
    log_path: &Path,
    full: bool,
    agent_name: Option<&str>,
) -> Option<String> {
    const MAX_MESSAGE_CHARS: usize = 1_000;
    const MAX_OUTPUT_CHARS: usize = 8_000;
    const HEAD_MESSAGE_COUNT: usize = 3;
    const TAIL_MESSAGE_COUNT: usize = 7;

    let content = std::fs::read_to_string(log_path).ok()?;
    let mut messages = collect_messages(&content);
    if messages.is_empty() {
        if let Some(notice) = unrecognized_json_log_notice(&content, log_path, agent_name) {
            return Some(notice);
        }
        return None;
    }
    if !full {
        truncate_messages(&mut messages, MAX_MESSAGE_CHARS);
        messages = cap_message_count(messages, HEAD_MESSAGE_COUNT, TAIL_MESSAGE_COUNT);
    }
    Some(join_messages(messages, full, MAX_OUTPUT_CHARS))
}

pub(crate) fn unrecognized_json_log_notice(
    content: &str,
    log_path: &Path,
    agent_name: Option<&str>,
) -> Option<String> {
    let mut total_lines = 0;
    let mut json_lines = 0;
    let mut first_sample: Option<String> = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || is_non_output_line(trimmed) {
            continue;
        }
        total_lines += 1;
        if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
            json_lines += 1;
            if first_sample.is_none() {
                first_sample = Some(trimmed.to_string());
            }
        }
    }

    if total_lines > 0 && json_lines == total_lines {
        let sample = first_sample.unwrap_or_default();
        let sample_truncated = if sample.chars().count() > 120 {
            format!("{}…", sample.chars().take(120).collect::<String>())
        } else {
            sample
        };
        let agent = agent_name.unwrap_or("unknown agent");
        Some(format!(
            "{UNRECOGNIZED_JSON_NOTICE_PREFIX} from {agent}]\nSample line: {sample_truncated}\nSee transcript at {}",
            log_path.display()
        ))
    } else {
        None
    }
}

pub(crate) fn is_aid_sentinel_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("=== AID TASK") || trimmed.starts_with("=== AID")
}

/// Diagnostic warning emitted by CLI runners (such as Claude or Codex) when max turns are reached.
/// This is runner state rather than task deliverable output.
pub(crate) fn is_agent_runner_warning_line(line: &str) -> bool {
    line.trim().starts_with("Warning: Reached maximum")
}

pub(crate) fn is_non_output_line(line: &str) -> bool {
    is_aid_sentinel_line(line) || is_agent_runner_warning_line(line)
}

pub(crate) fn extract_messages_research(log_path: &Path) -> Option<String> {
    const MAX_MESSAGE_CHARS: usize = 4_000;
    const MAX_OUTPUT_CHARS: usize = 20_000;

    let content = std::fs::read_to_string(log_path).ok()?;
    let mut messages = collect_messages(&content);
    if messages.is_empty() {
        return None;
    }
    truncate_messages(&mut messages, MAX_MESSAGE_CHARS);
    Some(join_messages(messages, false, MAX_OUTPUT_CHARS))
}

fn truncate_messages(messages: &mut [String], max_chars: usize) {
    for message in messages {
        if message.len() > max_chars {
            message.truncate(message.floor_char_boundary(max_chars.saturating_sub(3)));
            message.push_str("...");
        }
    }
}

fn cap_message_count(messages: Vec<String>, head: usize, tail: usize) -> Vec<String> {
    if messages.len() <= head + tail {
        return messages;
    }
    let omitted = messages.len() - head - tail;
    let mut capped = Vec::with_capacity(head + tail + 1);
    capped.extend(messages[..head].iter().cloned());
    capped.push(format!("[... {omitted} messages omitted ...]"));
    capped.extend(messages[messages.len() - tail..].iter().cloned());
    capped
}

fn join_messages(messages: Vec<String>, full: bool, max_output_chars: usize) -> String {
    let mut output = messages.join("\n---\n");
    if !full && output.len() > max_output_chars {
        output.truncate(output.floor_char_boundary(max_output_chars.saturating_sub(3)));
        output.push_str("...");
    }
    output
}
pub fn read_task_output(task: &Task) -> Result<String> {
    // Only paths proven to belong to this task (effective dir/worktree/task_dir or absolute declare).
    // Never resolve relative `-o` against process CWD or the shared repo root.
    if let Some(path) = super::show_output_owned::owned_output_path(task) {
        if let Some(content) = read_output_file(&path, task.agent) {
            return Ok(content);
        }
    }
    let persisted = paths::task_dir(task.id.as_str()).join("result.md");
    if let Some(content) = read_output_file(&persisted, task.agent) {
        return Ok(content);
    }
    Err(anyhow::anyhow!("Task has no output file"))
}

fn read_output_file(path: &Path, agent: AgentKind) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let content = match crate::agent::extract_response(agent, &raw) {
        Some(response) => response,
        None => raw,
    };
    is_valid_output_content(&content).then_some(content)
}

fn is_valid_output_content(content: &str) -> bool {
    content
        .lines()
        .any(|line| !line.trim().is_empty() && !is_non_output_line(line))
}

pub fn log_text(task_id: &str) -> Result<String> {
    let path = paths::log_path(task_id);
    std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read log file {}", path.display()))
}

pub fn log_text_brief(task_id: &str) -> Result<String> {
    let path = paths::log_path(task_id);
    Ok(read_tail(&path, 200, "No log available"))
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

pub(crate) fn tail_lines(content: &str, limit: usize) -> String {
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

#[cfg(test)]
mod notice_delivery_tests {
    use super::unrecognized_json_log_notice;

    #[test]
    fn an_unparsed_stream_produces_a_nonempty_notice() {
        // The round trip that matters: aid cannot parse an agent's envelope, says so,
        // and the delivery guard reads that as work delivered. Treating it as no
        // delivery is what recorded a completed 18-minute cross-audit as FAILED
        // (t-d1f7374e) while its transcript held the finished report.
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("t-x.jsonl");
        std::fs::write(
            &log,
            "{\"type\":\"event\",\"event\":{\"type\":\"unknown_shape\"}}\n\
             === AID TASK t-x DONE (exit 0) ===\n",
        )
        .unwrap();

        let notice = unrecognized_json_log_notice(
            &std::fs::read_to_string(&log).unwrap(),
            &log,
            Some("commandcode"),
        )
        .expect("an all-JSON stream with no recognised arm must produce a notice");

        assert!(notice.contains("commandcode"), "notice must name the agent: {notice}");
        assert!(!notice.trim().is_empty());
    }
}
