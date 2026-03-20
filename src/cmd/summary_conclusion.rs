// Extract conclusion text for task summaries.
// Exports: extract_conclusion().
// Deps: crate::types::Task, serde_json.
use crate::types::Task;
use serde_json::Value;
use std::path::Path;

pub(crate) fn extract_conclusion(task: &Task) -> String {
    if let Some(conclusion) = task.output_path.as_deref().and_then(read_conclusion_from_output) {
        return conclusion;
    }
    task.log_path
        .as_deref()
        .and_then(read_conclusion_from_log)
        .unwrap_or_default()
}

fn read_conclusion_from_output(path: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    extract_last_text_block(&content).map(|s| truncate_conclusion(&s))
}

fn read_conclusion_from_log(path: &str) -> Option<String> {
    let content = std::fs::read_to_string(Path::new(path)).ok()?;
    extract_last_log_message(&content).map(|message| truncate_conclusion(&message))
}

fn extract_last_log_message(content: &str) -> Option<String> {
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
    messages
        .into_iter()
        .rev()
        .find_map(|message| extract_last_text_block(&message))
}

fn extract_last_text_block(content: &str) -> Option<String> {
    let sections: Vec<&str> = content.split("\n---\n").collect();
    sections
        .iter()
        .rev()
        .find_map(|section| {
            let paragraphs: Vec<&str> = section.split("\n\n").collect();
            paragraphs
                .iter()
                .rev()
                .find_map(|paragraph| normalize_text(paragraph).filter(|text| !text.is_empty()))
        })
}

fn normalize_text(text: &str) -> Option<String> {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn truncate_conclusion(text: &str) -> String {
    if text.len() <= 2_000 {
        text.to_string()
    } else {
        let end = text.floor_char_boundary(2_000 - 3);
        format!("{}...", &text[..end])
    }
}
