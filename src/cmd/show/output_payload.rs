// Agent event payload extraction for show message collection.
// Deps: serde_json::Value; consumed by the parent message collector.

use serde_json::Value;

pub(super) fn completed_agent_message(value: &Value) -> Option<String> {
    let item = value.get("item")?;
    let is_agent_message = item.get("type").and_then(|kind| kind.as_str()) == Some("agent_message");
    let text = item.get("text").and_then(|text| text.as_str())?;
    is_agent_message.then(|| text.to_string())
}

pub(super) fn assistant_message_text(value: &Value) -> Option<String> {
    value.get("content").and_then(extract_text_payload)
}

pub(super) fn assistant_event_text(value: &Value) -> Option<String> {
    value
        .pointer("/message/content")
        .and_then(extract_text_payload)
}

pub(super) fn text_event_message(value: &Value) -> Option<String> {
    value
        .pointer("/part/text")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

pub(super) fn replaceable_text_event(value: &Value) -> Option<String> {
    value
        .get("content")
        .and_then(extract_text_payload)
        .or_else(|| value.get("text").and_then(extract_text_payload))
}

pub(super) fn error_event_message(value: &Value) -> Option<String> {
    [
        value.get("message"),
        value.pointer("/error/message"),
        value.pointer("/error/details/0/message"),
    ]
    .into_iter()
    .flatten()
    .find_map(extract_text_payload)
    .map(|message| format!("[error] {message}"))
}

pub(super) fn tool_event_message(value: &Value) -> Option<String> {
    let tool = tool_name(value).unwrap_or("tool");
    let error = [
        value.pointer("/part/state/error"),
        value.pointer("/error/message"),
        value.get("error"),
    ]
    .into_iter()
    .flatten()
    .find_map(extract_text_payload);
    if let Some(error) = error {
        return Some(format!("[{tool}] Error: {error}"));
    }

    let payload = [
        value.pointer("/part/state/output"),
        value.get("output"),
        value.get("arguments"),
        value.pointer("/functionCall/args"),
        value.get("parameters"),
        value.get("input"),
        value.pointer("/tool_call"),
    ]
    .into_iter()
    .flatten()
    .find_map(format_tool_payload);
    payload.map(|payload| format!("[{tool}] {payload}"))
}

/// Render a tool's argument payload as a short, readable line.
///
/// JSON dumps like `{"file_path":"/very/long/path","limit":250,"offset":450}`
/// are noisy in TUI/CLI output. When the payload has a single recognizable
/// "primary" key (file/path/pattern/command/url/...), surface that value with
/// a few salient extras (offset/limit/line_numbers) instead of the raw JSON.
/// Falls back to a length-capped JSON string for unknown shapes.
fn format_tool_payload(value: &Value) -> Option<String> {
    if let Some(text) = extract_text_payload(value) {
        return Some(truncate_payload(&text));
    }
    if let Value::Object(map) = value {
        const PRIMARY_KEYS: &[&str] = &[
            "file_path",
            "path",
            "directory_path",
            "url",
            "command",
            "pattern",
            "query",
            "prompt",
        ];
        for key in PRIMARY_KEYS {
            if let Some(primary) = map.get(*key).and_then(Value::as_str) {
                let mut out = primary.to_string();
                let extras: Vec<String> = map
                    .iter()
                    .filter(|(k, _)| k.as_str() != *key)
                    .filter_map(|(k, v)| {
                        let v_str = match v {
                            Value::String(s) => s.clone(),
                            Value::Number(n) => n.to_string(),
                            Value::Bool(b) => b.to_string(),
                            _ => return None,
                        };
                        Some(format!("{k}={v_str}"))
                    })
                    .collect();
                if !extras.is_empty() {
                    out.push_str(" (");
                    out.push_str(&extras.join(", "));
                    out.push(')');
                }
                return Some(truncate_payload(&out));
            }
        }
    }
    let raw = match value {
        Value::Null => return None,
        other => other.to_string(),
    };
    Some(truncate_payload(&raw))
}

fn truncate_payload(text: &str) -> String {
    const MAX_LEN: usize = 160;
    if text.chars().count() <= MAX_LEN {
        return text.to_string();
    }
    let truncated: String = text.chars().take(MAX_LEN).collect();
    format!("{truncated}…")
}

fn tool_name(value: &Value) -> Option<&str> {
    value
        .get("tool_name")
        .and_then(Value::as_str)
        .or_else(|| value.get("name").and_then(Value::as_str))
        .or_else(|| value.get("toolName").and_then(Value::as_str))
        .or_else(|| value.pointer("/part/tool").and_then(Value::as_str))
        .or_else(|| value.pointer("/functionCall/name").and_then(Value::as_str))
        .or_else(|| value.get("tool").and_then(Value::as_str))
        .or_else(|| value.pointer("/tool/name").and_then(Value::as_str))
        .or_else(|| value.pointer("/data/toolName").and_then(Value::as_str))
}

pub(super) fn copilot_delta_text(value: &Value) -> Option<String> {
    value
        .pointer("/data/deltaContent")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

pub(super) fn text_delta_text(value: &Value) -> Option<String> {
    value
        .get("delta")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/data/deltaContent").and_then(Value::as_str))
        .map(ToOwned::to_owned)
}

pub(super) fn result_event_text(value: &Value) -> Option<String> {
    final_text_field(value).or_else(|| duplicate_prone_result_text(value))
}

/// Commandcode (and similar) terminal field — distinct from prior assistant text.
pub(super) fn final_text_field(value: &Value) -> Option<String> {
    value
        .get("finalText")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

/// Claude/qwen top-level `result` (and `/result/text`) — may repeat assistant text.
pub(super) fn duplicate_prone_result_text(value: &Value) -> Option<String> {
    value
        .pointer("/result/text")
        .and_then(Value::as_str)
        .or_else(|| value.get("result").and_then(Value::as_str))
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

pub(super) fn copilot_tool_start_message(value: &Value) -> Option<String> {
    let tool = value.pointer("/data/toolName").and_then(Value::as_str)?;
    let payload = value
        .pointer("/data/arguments")
        .and_then(stringify_payload)?;
    Some(format!("[{tool}] {payload}"))
}

pub(super) fn copilot_tool_error_message(value: &Value) -> Option<String> {
    if value.pointer("/data/success").and_then(Value::as_bool) != Some(false) {
        return None;
    }
    let tool = value
        .pointer("/data/toolName")
        .and_then(Value::as_str)
        .unwrap_or("tool");
    let message = value
        .pointer("/data/error")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/data/result/error").and_then(Value::as_str))
        .unwrap_or("unknown error");
    Some(format!("[{tool}] Error: {message}"))
}

pub(super) fn extract_text_payload(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let parts = items
                .iter()
                .filter_map(extract_text_payload)
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.concat())
        }
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("thinking") {
                return None;
            }
            for key in ["text", "content", "parts", "finalText"] {
                if let Some(text) = map.get(key).and_then(extract_text_payload)
                    && !text.is_empty()
                {
                    return Some(text);
                }
            }
            None
        }
        _ => None,
    }
}

fn stringify_payload(value: &Value) -> Option<String> {
    extract_text_payload(value).or_else(|| match value {
        Value::Null => None,
        other => Some(other.to_string()),
    })
}
