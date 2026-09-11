// Cursor stream-json event parsing and tool metadata helpers.
// Exports: parse_json_event; deps: agent truncation, task types, chrono, serde_json.

use chrono::Local;
use serde_json::json;
use crate::types::*;
use crate::agent::truncate::capped_detail_with;

pub(super) fn parse_json_event(
    task_id: &TaskId,
    v: &serde_json::Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let event_type = v.get("type").and_then(|value| value.as_str())?;
    let (event_kind, detail, metadata) = match event_type {
        "system" => parse_system_event(v),
        "assistant" => {
            let detail = v
                .pointer("/message/content/0/text")
                .and_then(|value| value.as_str())?
                .to_string();
            (EventKind::Reasoning, detail, None)
        }
        "thinking" => {
            // Skip thinking deltas — they're tiny streaming fragments, not useful events
            return None;
        }
        "tool_call" => parse_tool_call(v)?,
        "result" => parse_result_event(v),
        "error" => {
            let detail = v
                .get("message")
                .or_else(|| v.get("detail"))
                .or_else(|| v.get("error"))
                .and_then(|value| value.as_str())
                .unwrap_or("unknown error")
                .to_string();
            (EventKind::Error, detail, None)
        }
        _ => return None,
    };
    let (detail, metadata) = capped_detail_with(&detail, metadata);
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind,
        detail,
        metadata,
    })
}

fn parse_system_event(
    value: &serde_json::Value,
) -> (EventKind, String, Option<serde_json::Value>) {
    let subtype = value.get("subtype").and_then(|value| value.as_str()).unwrap_or("system");
    let model = value.get("model").and_then(|value| value.as_str());
    let session_id = value.get("session_id").and_then(|value| value.as_str());
    let detail = model
        .map(|model| format!("{subtype}: {model}"))
        .unwrap_or_else(|| subtype.to_string());
    let metadata = match (model, session_id) {
        (None, None) => None,
        _ => {
            let mut metadata = json!({});
            if let Some(model) = model { metadata["model"] = json!(model); }
            if let Some(session_id) = session_id {
                metadata["agent_session_id"] = json!(session_id);
            }
            Some(metadata)
        }
    };
    (EventKind::Reasoning, detail, metadata)
}

fn parse_result_event(
    value: &serde_json::Value,
) -> (EventKind, String, Option<serde_json::Value>) {
    let input = usage_i64(value, "inputTokens");
    let output = usage_i64(value, "outputTokens");
    let cached = usage_i64(value, "cacheReadTokens");
    let total = input + output + cached;
    let detail = format!("tokens: {input} in + {output} out = {total} ({cached} cached)");
    let mut metadata = json!({
        "tokens": total,
        "input_tokens": input,
        "output_tokens": output,
        "prompt_tokens": input,
    });
    if let Some(cost) = value.pointer("/usage/totalCostUSD").and_then(|value| value.as_f64()) {
        metadata["cost_usd"] = json!(cost);
    }
    (EventKind::Completion, detail, Some(metadata))
}

fn usage_i64(value: &serde_json::Value, key: &str) -> i64 {
    value.pointer(&format!("/usage/{key}")).and_then(|value| value.as_i64()).unwrap_or(0)
}

fn parse_tool_call(
    value: &serde_json::Value,
) -> Option<(EventKind, String, Option<serde_json::Value>)> {
    let subtype = value.get("subtype").and_then(|value| value.as_str()).unwrap_or("call");
    let calls = value.get("tool_call").and_then(|value| value.as_object())?;
    let (tool_name, tool_data) = calls
        .iter()
        .find(|(key, data)| key.ends_with("ToolCall") && data.is_object())?;
    let path = tool_path(tool_data);
    let argument = match tool_name.as_str() {
        "globToolCall" => tool_argument(tool_data, &["globPattern", "pattern"], "*").to_string(),
        "grepToolCall" => tool_argument(tool_data, &["pattern"], "?").to_string(),
        "shellToolCall" | "terminalToolCall" => {
            tool_argument(tool_data, &["command"], "?").to_string()
        }
        "writeToolCall" | "editToolCall" | "deleteToolCall" | "readToolCall" => {
            path.to_string()
        }
        _ => unknown_tool_key(tool_name, tool_data),
    };
    let action = match tool_name.as_str() {
        "writeToolCall" => "write",
        "editToolCall" => "edit",
        "deleteToolCall" => "delete",
        "readToolCall" => "read",
        "globToolCall" => "glob",
        "grepToolCall" => "grep",
        "shellToolCall" | "terminalToolCall" => "shell",
        _ => tool_name,
    };
    let kind = match tool_name.as_str() {
        "writeToolCall" | "editToolCall" | "deleteToolCall" => EventKind::FileWrite,
        "readToolCall" => EventKind::FileRead,
        _ => EventKind::ToolCall,
    };
    let metadata = match kind {
        EventKind::FileWrite | EventKind::FileRead => Some(json!({ "files": [&argument] })),
        EventKind::ToolCall => Some(json!({ "command": &argument })),
        _ => None,
    };
    Some((kind, format!("{subtype}: {action} {argument}"), metadata))
}

fn unknown_tool_key(tool_name: &str, value: &serde_json::Value) -> String {
    let arguments = value.get("args").unwrap_or(&serde_json::Value::Null);
    format!("{tool_name}:{arguments}")
}

fn tool_path(value: &serde_json::Value) -> &str {
    value
        .pointer("/args/path")
        .or_else(|| value.pointer("/args/filePath"))
        .and_then(|value| value.as_str())
        .unwrap_or("?")
}

fn tool_argument<'a>(value: &'a serde_json::Value, keys: &[&str], fallback: &'a str) -> &'a str {
    keys.iter()
        .find_map(|key| value.pointer(&format!("/args/{key}")).and_then(|value| value.as_str()))
        .unwrap_or(fallback)
}
