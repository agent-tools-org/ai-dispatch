// Droid streaming event decoding and event-specific details.
// Depends on serde_json, task types, and shared truncation helpers.

use super::super::truncate::{capped_detail, capped_detail_with};
use crate::types::*;
use chrono::Local;
use serde_json::{Value, json};

pub(super) fn parse_event(task_id: &TaskId, line: &str) -> Option<TaskEvent> {
    let v: Value = serde_json::from_str(line).ok()?;
    let now = Local::now();
    let event_type = v.get("type")?.as_str()?;
    match event_type {
        "assistant_message" | "text" => parse_text(task_id, &v, now),
        // Only emit on the request side. droid stream-json fires both
        // `tool_call` (the model's request) and `tool_result` (the tool's
        // response) for one logical operation, plus sometimes `tool_use`
        // as an alias. Keep only `tool_call` so one operation produces
        // one event.
        "tool_call" => parse_tool_call(task_id, &v, now),
        "tool_use" | "tool_result" => None,
        "mission_step" => parse_mission_step(task_id, &v, now),
        "session_forked" => parse_session_forked(task_id, &v, now),
        "usage" | "turn_complete" => parse_usage(task_id, &v, now),
        "error" => parse_error_event(task_id, &v, now),
        _ => None,
    }
}

fn parse_mission_step(task_id: &TaskId, v: &Value, now: chrono::DateTime<Local>) -> Option<TaskEvent> {
    let description = v.get("description")?.as_str()?.trim();
    if description.is_empty() {
        return None;
    }
    let text = match v.get("step").and_then(|value| value.as_str()) {
        Some(step) if !step.is_empty() => format!("{step} {description}"),
        _ => description.to_string(),
    };
    let (detail, metadata) = capped_detail(&text);
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Milestone,
        detail,
        metadata,
    })
}

fn parse_session_forked(task_id: &TaskId, v: &Value, now: chrono::DateTime<Local>) -> Option<TaskEvent> {
    let new_id = v.get("new_id")?.as_str()?;
    let detail = match v.get("parent_id").and_then(|value| value.as_str()) {
        Some(parent_id) if !parent_id.is_empty() => format!("forked {new_id} from {parent_id}"),
        _ => format!("forked {new_id}"),
    };
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Milestone,
        detail,
        metadata: Some(json!({ "agent_session_id": new_id })),
    })
}

fn parse_error_event(task_id: &TaskId, v: &Value, now: chrono::DateTime<Local>) -> Option<TaskEvent> {
    crate::routing::quota::channel::mark_stream_refusal(AgentKind::Droid, None, &v.to_string());
    let (detail, metadata) = capped_detail(droid_error_detail(v).as_deref().unwrap_or("unknown error"));
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Error,
        detail,
        metadata,
    })
}

fn droid_error_detail(v: &Value) -> Option<String> {
    let message = v
        .get("message")
        .and_then(|value| value.as_str())
        .or_else(|| v.get("error").and_then(|value| value.as_str()))
        .or_else(|| v.pointer("/error/message").and_then(|value| value.as_str()));
    if let Some(message) = message
        && !message.is_empty()
    {
        return Some(message.to_string());
    }
    v.get("error_type")
        .and_then(|value| value.as_str())
        .or_else(|| v.pointer("/error/type").and_then(|value| value.as_str()))
        .map(ToOwned::to_owned)
}

fn parse_tool_call(task_id: &TaskId, v: &Value, now: chrono::DateTime<Local>) -> Option<TaskEvent> {
    let name = v
        .get("toolName")
        .or_else(|| v.get("toolId"))
        .or_else(|| v.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("tool");

    let signature = tool_signature(v, name);

    let text = match &signature {
        Some(sig) => format!("{} {}", name, sig),
        None => name.to_string(),
    };

    let metadata = signature.as_ref().map(|s| json!({ "command": s }));
    let (detail, metadata) = capped_detail_with(&text, metadata);

    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::ToolCall,
        detail,
        metadata,
    })
}

fn parse_text(task_id: &TaskId, v: &Value, now: chrono::DateTime<Local>) -> Option<TaskEvent> {
    let text = v
        .get("content")
        .or_else(|| v.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("");
    if text.is_empty() {
        return None;
    }
    let (detail, metadata) = capped_detail(text);
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Reasoning,
        detail,
        metadata,
    })
}

fn parse_usage(task_id: &TaskId, v: &Value, now: chrono::DateTime<Local>) -> Option<TaskEvent> {
    let input = v.get("input_tokens").and_then(|t| t.as_i64()).unwrap_or(0);
    let output = v.get("output_tokens").and_then(|t| t.as_i64()).unwrap_or(0);
    let total = input + output;
    let cost = v.get("cost_usd").and_then(|c| c.as_f64());
    let model = v.get("model").and_then(|m| m.as_str()).map(ToOwned::to_owned);
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Completion,
        detail: format!("tokens: {input} in + {output} out = {total}"),
        metadata: Some(json!({
            "tokens": total, "input_tokens": input, "output_tokens": output,
            "model": model, "cost_usd": cost,
        })),
    })
}

fn tool_signature(v: &Value, name: &str) -> Option<String> {
    // droid may include parameters under several keys; try common ones.
    let params = v
        .get("parameters")
        .or_else(|| v.get("arguments"))
        .or_else(|| v.get("input"))
        .or_else(|| v.get("args"));

    params.and_then(|p| parameter_signature(p, name))
}

fn first_parameter(p: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(val) = p.get(*k).and_then(|v| v.as_str()) {
            return Some(val.to_string());
        }
    }
    None
}

fn parameter_signature(p: &Value, name: &str) -> Option<String> {
    let mut signature: Option<String> = None;
    match name.to_lowercase().as_str() {
        // file operations: prefer explicit file path keys
        "read" | "readtool" | "readtoolcall" | "read_tool" => {
            if let Some(path) = first_parameter(p, &["file_path", "path", "filePath"]) {
                signature = Some(path);
            }
        }
        "write" | "edit" | "writetool" | "write_tool" => {
            if let Some(path) = first_parameter(p, &["file_path", "path", "filePath"]) {
                signature = Some(path);
            }
        }
        "grep" | "greet" => {
            let pattern = first_parameter(p, &["pattern", "globPattern", "query"]);
            let path = first_parameter(p, &["path", "targetDirectory", "file_path", "filePath"]);
            signature = match (pattern, path) {
                (Some(pat), Some(pth)) => Some(format!("{pat} {pth}")),
                (Some(pat), None) => Some(pat),
                (None, Some(pth)) => Some(pth),
                _ => None,
            };
        }
        "bash" | "shell" | "execute" | "sh" | "command" => {
            if let Some(cmd) = first_parameter(p, &["command", "cmd", "shell"]) {
                signature = Some(cmd);
            }
        }
        _ => {
            // Fallback: pick any string-ish parameter if present
            if let Some((k, v)) = p.as_object().and_then(|o| o.iter().next()) {
                if let Some(s) = v.as_str() {
                    signature = Some(s.to_string());
                } else {
                    signature = Some(k.to_string());
                }
            }
        }
    }
    signature
}
