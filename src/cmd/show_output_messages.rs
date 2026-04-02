// Output and log rendering helpers for `aid show`.
// Exports: output_text, output_text_brief, output_text_full, log_text, read_task_output, read_tail.
// Deps: paths, Store, Task, serde_json::Value.
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::paths;
use crate::store::Store;
use crate::types::Task;

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

#[allow(dead_code)]
pub fn output_text_full(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = super::super::load_task(store, task_id)?;
    if let Ok(content) = read_task_output(&task) {
        return Ok(content);
    }
    if let Some(content) = extract_messages_for_task(&task, task_id, true) {
        return Ok(content);
    }
    let path = task_log_path(&task, task_id);
    Ok(read_tail(&path, 200, "No output or log available"))
}

fn render_task_output(task: &Task, task_id: &str, full: bool, tail_lines: usize) -> Result<String> {
    if let Ok(content) = read_task_output(task) {
        return Ok(content);
    }
    if !full && is_research_task(task) {
        let path = task_log_path(task, task_id);
        if let Some(content) = extract_messages_research(&path) {
            return Ok(content);
        }
    }
    if let Some(content) = extract_messages_for_task(task, task_id, full) {
        return Ok(content);
    }
    let path = task_log_path(task, task_id);
    Ok(read_tail(&path, tail_lines, "No output or log available"))
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

fn extract_messages_for_task(task: &Task, task_id: &str, full: bool) -> Option<String> {
    extract_messages_from_log(&task_log_path(task, task_id), full)
}

pub(crate) fn extract_messages_from_log(log_path: &Path, full: bool) -> Option<String> {
    const MAX_MESSAGE_CHARS: usize = 1_000;
    const MAX_OUTPUT_CHARS: usize = 8_000;
    const HEAD_MESSAGE_COUNT: usize = 3;
    const TAIL_MESSAGE_COUNT: usize = 7;

    let content = std::fs::read_to_string(log_path).ok()?;
    let mut messages = collect_messages(&content);
    if messages.is_empty() {
        return None;
    }
    if !full {
        truncate_messages(&mut messages, MAX_MESSAGE_CHARS);
        messages = cap_message_count(messages, HEAD_MESSAGE_COUNT, TAIL_MESSAGE_COUNT);
    }
    Some(join_messages(messages, full, MAX_OUTPUT_CHARS))
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

fn collect_messages(content: &str) -> Vec<String> {
    let mut messages = Vec::new();
    let mut streaming_message = String::new();
    for line in content.lines() {
        let cleaned = strip_ansi(line);
        let Ok(value) = serde_json::from_str::<Value>(&cleaned) else {
            continue;
        };
        collect_message(&value, &mut messages, &mut streaming_message);
    }
    if !streaming_message.is_empty() {
        messages.push(streaming_message);
    }
    messages
}

fn collect_message(value: &Value, messages: &mut Vec<String>, streaming_message: &mut String) {
    match value.get("type").and_then(|kind| kind.as_str()) {
        Some("item.completed") => push_optional(messages, completed_agent_message(value)),
        Some("message") => collect_assistant_message(value, messages, streaming_message),
        Some("assistant") => {
            let text = value
                .pointer("/message/content/0/text")
                .and_then(|text| text.as_str());
            if let Some(text) = text {
                streaming_message.push_str(text);
            }
        }
        Some("text") => push_flushed_message(messages, text_message(value), streaming_message),
        Some("tool_use") => collect_tool_use_message(value, messages, streaming_message),
        _ => {}
    }
}

fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            let mut j = i + 2;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b';') {
                j += 1;
            }
            if j < bytes.len() && bytes[j].is_ascii_alphabetic() {
                i = j + 1;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn completed_agent_message(value: &Value) -> Option<String> {
    let item = value.get("item")?;
    let is_agent_message = item.get("type").and_then(|kind| kind.as_str()) == Some("agent_message");
    let text = item.get("text").and_then(|text| text.as_str())?;
    is_agent_message.then(|| text.to_string())
}

fn collect_assistant_message(
    value: &Value,
    messages: &mut Vec<String>,
    streaming_message: &mut String,
) {
    let is_assistant = value.get("role").and_then(|role| role.as_str()) == Some("assistant");
    let Some(content) = value.get("content").and_then(|text| text.as_str()) else {
        return;
    };
    if !is_assistant {
        return;
    }
    if value.get("delta").and_then(|delta| delta.as_bool()) == Some(true) {
        streaming_message.push_str(content);
    } else {
        push_flushed_message(messages, Some(content), streaming_message);
    }
}

fn collect_tool_use_message(
    value: &Value,
    messages: &mut Vec<String>,
    streaming_message: &mut String,
) {
    let tool = value
        .pointer("/part/tool")
        .and_then(|tool| tool.as_str())
        .unwrap_or("tool");
    let status = value
        .pointer("/part/state/status")
        .and_then(|status| status.as_str());
    let detail = match status {
        Some("error") => {
            let error = value
                .pointer("/part/state/error")
                .and_then(|error| error.as_str())
                .unwrap_or("error");
            format!("[{tool}] Error: {error}")
        }
        _ => {
            let output = value
                .pointer("/part/state/output")
                .and_then(|output| output.as_str())
                .unwrap_or("");
            if output.is_empty() {
                return;
            }
            format!("[{tool}] {output}")
        }
    };
    if !streaming_message.is_empty() {
        messages.push(std::mem::take(streaming_message));
    }
    messages.push(detail);
}

fn push_flushed_message(
    messages: &mut Vec<String>,
    text: Option<&str>,
    streaming_message: &mut String,
) {
    let Some(text) = text else {
        return;
    };
    if !streaming_message.is_empty() {
        messages.push(std::mem::take(streaming_message));
    }
    messages.push(text.to_string());
}

fn push_optional(messages: &mut Vec<String>, message: Option<String>) {
    if let Some(message) = message {
        messages.push(message);
    }
}

fn text_message(value: &Value) -> Option<&str> {
    value
        .get("content")
        .and_then(|text| text.as_str())
        .or_else(|| value.get("text").and_then(|text| text.as_str()))
        .or_else(|| value.pointer("/part/text").and_then(|text| text.as_str()))
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
    if let Some(path) = task.output_path.as_deref() {
        return std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read output file {path}"));
    }
    let persisted = paths::task_dir(task.id.as_str()).join("result.md");
    if persisted.exists() {
        return std::fs::read_to_string(&persisted)
            .with_context(|| format!("Failed to read result file {}", persisted.display()));
    }
    Err(anyhow::anyhow!("Task has no output file"))
}

pub fn log_text(task_id: &str) -> Result<String> {
    let path = paths::log_path(task_id);
    std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read log file {}", path.display()))
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
