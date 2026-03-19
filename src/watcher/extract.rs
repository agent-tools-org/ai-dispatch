// Watcher extraction helpers for milestone and finding tags.
// Exports parsing and broadcast utilities used by streaming and tests.

use chrono::Local;

use crate::types::{EventKind, TaskEvent, TaskId};

const MILESTONE_TAG: &str = "[MILESTONE]";
const FINDING_TAG: &str = "[FINDING]";

pub(super) fn parse_milestone_event(task_id: &TaskId, line: &str) -> Option<TaskEvent> {
    let detail = extract_milestone_detail(line)?;
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Milestone,
        detail,
        metadata: None,
    })
}

pub(super) fn extract_milestone_detail(line: &str) -> Option<String> {
    if !line.contains(MILESTONE_TAG) {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(line)
        && let Some(detail) = extract_milestone_from_json(&value)
    {
        return Some(detail);
    }
    let trimmed = line.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return None;
    }
    extract_milestone_from_text(line)
}

pub(super) fn extract_finding_detail(line: &str) -> Option<String> {
    if !line.contains(FINDING_TAG) {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(line)
        && let Some(detail) = extract_finding_from_json(&value)
    {
        return Some(detail);
    }
    let trimmed = line.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return None;
    }
    extract_finding_from_text(line)
}

pub(super) fn append_to_broadcast(workgroup_id: &str, task_id: &str, content: &str) {
    let Ok(broadcast_path) = crate::paths::workspace_dir(workgroup_id).map(|path| path.join("broadcast.md")) else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&broadcast_path)
    {
        use std::io::Write;
        let timestamp = Local::now().format("%H:%M:%S");
        let _ = writeln!(file, "- [{timestamp}] ({task_id}) {content}");
    }
}

fn extract_milestone_from_json(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => extract_milestone_from_text(text),
        serde_json::Value::Array(items) => items.iter().find_map(extract_milestone_from_json),
        serde_json::Value::Object(map) => map.values().find_map(extract_milestone_from_json),
        _ => None,
    }
}

fn extract_milestone_from_text(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let tag_pos = line.find(MILESTONE_TAG)?;
        if tag_is_inside_code_string(line, tag_pos) {
            return None;
        }
        let detail = line[tag_pos + MILESTONE_TAG.len()..]
            .trim()
            .trim_start_matches(':')
            .trim();
        if detail.is_empty() {
            None
        } else {
            Some(detail.to_string())
        }
    })
}

fn extract_finding_from_json(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => extract_finding_from_text(text),
        serde_json::Value::Array(items) => items.iter().find_map(extract_finding_from_json),
        serde_json::Value::Object(map) => map.values().find_map(extract_finding_from_json),
        _ => None,
    }
}

fn extract_finding_from_text(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let tag_pos = line.find(FINDING_TAG)?;
        if tag_is_inside_code_string(line, tag_pos) {
            return None;
        }
        let detail = line[tag_pos + FINDING_TAG.len()..]
            .trim()
            .trim_start_matches(':')
            .trim();
        if detail.is_empty() {
            None
        } else {
            Some(detail.to_string())
        }
    })
}

fn tag_is_inside_code_string(line: &str, tag_pos: usize) -> bool {
    let before = &line[..tag_pos];
    let single_quotes = before.chars().filter(|&c| c == '\'').count();
    let double_quotes = before.chars().filter(|&c| c == '"').count();
    if single_quotes % 2 == 1 || double_quotes % 2 == 1 {
        return true;
    }
    let trimmed = line.trim_start();
    if trimmed.starts_with("```") || trimmed.starts_with("///") {
        return true;
    }
    if trimmed.starts_with("println!")
        || trimmed.starts_with("eprintln!")
        || trimmed.starts_with("console.log")
    {
        return true;
    }
    false
}
