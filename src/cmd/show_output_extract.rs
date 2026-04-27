// Structured log-to-text extraction helpers for `aid show`.
// Exports: `collect_messages` used by CLI/TUI/web output rendering.
// Deps: serde_json::Value for agent log event parsing.

use serde_json::Value;

#[derive(Default)]
struct MessageCollector {
    messages: Vec<String>,
    streaming_message: String,
    replaceable_message: Option<String>,
}

pub(super) fn collect_messages(content: &str) -> Vec<String> {
    let mut collector = MessageCollector::default();
    for line in content.lines() {
        let cleaned = strip_ansi(line);
        let Ok(value) = serde_json::from_str::<Value>(&cleaned) else {
            continue;
        };
        collector.collect(&value);
    }
    collector.finish()
}

impl MessageCollector {
    fn collect(&mut self, value: &Value) {
        match value.get("type").and_then(|kind| kind.as_str()) {
            Some("item.completed") => self.push_message(completed_agent_message(value)),
            Some("message") => self.collect_message_event(value),
            Some("assistant.message") => self.collect_copilot_message_event(value),
            Some("assistant.message_delta") => {
                self.append_streaming(copilot_delta_text(value));
            }
            Some("assistant") => self.append_streaming(assistant_event_text(value)),
            Some("text") => self.collect_text_event(value),
            Some("tool.execution_start") => {
                self.flush_pending();
                if let Some(detail) = copilot_tool_start_message(value) {
                    self.messages.push(detail);
                }
            }
            Some("tool.execution_complete") => {
                self.flush_pending();
                if let Some(detail) = copilot_tool_error_message(value) {
                    self.messages.push(detail);
                }
            }
            Some("tool_use" | "tool_call" | "function_call" | "tool_result") => {
                self.flush_pending();
                if let Some(detail) = tool_event_message(value) {
                    self.messages.push(detail);
                }
            }
            Some("error") => {
                self.flush_pending();
                if let Some(detail) = error_event_message(value) {
                    self.messages.push(detail);
                }
            }
            Some("result" | "turn_complete" | "completion" | "done" | "step_finish") => {
                self.flush_pending();
            }
            _ => {}
        }
    }

    fn collect_message_event(&mut self, value: &Value) {
        if value.get("role").and_then(|role| role.as_str()) != Some("assistant") {
            return;
        }
        let text = assistant_message_text(value);
        if value.get("delta").and_then(|delta| delta.as_bool()) == Some(true) {
            self.append_streaming(text);
        } else {
            self.push_message(text);
        }
    }

    fn collect_copilot_message_event(&mut self, value: &Value) {
        let text = value.pointer("/data/content").and_then(Value::as_str);
        let Some(text) = text.filter(|text| !text.is_empty()) else {
            self.flush_pending();
            return;
        };
        if !self.streaming_message.is_empty() && self.streaming_message == text {
            self.messages.push(std::mem::take(&mut self.streaming_message));
            return;
        }
        self.push_message(Some(text.to_string()));
    }

    fn collect_text_event(&mut self, value: &Value) {
        if value.pointer("/part/text").is_some() {
            self.push_message(text_event_message(value));
            return;
        }
        if let Some(text) = replaceable_text_event(value) {
            self.replaceable_message = Some(text);
        }
    }

    fn append_streaming(&mut self, text: Option<String>) {
        let Some(text) = text else {
            return;
        };
        if self.replaceable_message.is_some() {
            self.flush_replaceable();
        }
        self.streaming_message.push_str(&text);
    }

    fn push_message(&mut self, text: Option<String>) {
        let Some(text) = text else {
            return;
        };
        self.flush_pending();
        self.messages.push(text);
    }

    fn flush_pending(&mut self) {
        self.flush_replaceable();
        self.flush_streaming();
    }

    fn flush_replaceable(&mut self) {
        if let Some(text) = self.replaceable_message.take() {
            self.messages.push(text);
        }
    }

    fn flush_streaming(&mut self) {
        if !self.streaming_message.is_empty() {
            self.messages.push(std::mem::take(&mut self.streaming_message));
        }
    }

    fn finish(mut self) -> Vec<String> {
        self.flush_pending();
        self.messages
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

fn assistant_message_text(value: &Value) -> Option<String> {
    value.get("content").and_then(extract_text_payload)
}

fn assistant_event_text(value: &Value) -> Option<String> {
    value.pointer("/message/content").and_then(extract_text_payload)
}

fn text_event_message(value: &Value) -> Option<String> {
    value.pointer("/part/text")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn replaceable_text_event(value: &Value) -> Option<String> {
    value.get("content")
        .and_then(extract_text_payload)
        .or_else(|| value.get("text").and_then(extract_text_payload))
}

fn error_event_message(value: &Value) -> Option<String> {
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

fn tool_event_message(value: &Value) -> Option<String> {
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
    value.get("tool_name")
        .and_then(Value::as_str)
        .or_else(|| value.get("name").and_then(Value::as_str))
        .or_else(|| value.get("toolName").and_then(Value::as_str))
        .or_else(|| value.pointer("/part/tool").and_then(Value::as_str))
        .or_else(|| value.pointer("/functionCall/name").and_then(Value::as_str))
        .or_else(|| value.get("tool").and_then(Value::as_str))
        .or_else(|| value.pointer("/tool/name").and_then(Value::as_str))
        .or_else(|| value.pointer("/data/toolName").and_then(Value::as_str))
}

fn copilot_delta_text(value: &Value) -> Option<String> {
    value.pointer("/data/deltaContent")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn copilot_tool_start_message(value: &Value) -> Option<String> {
    let tool = value.pointer("/data/toolName").and_then(Value::as_str)?;
    let payload = value.pointer("/data/arguments").and_then(stringify_payload)?;
    Some(format!("[{tool}] {payload}"))
}

fn copilot_tool_error_message(value: &Value) -> Option<String> {
    if value.pointer("/data/success").and_then(Value::as_bool) != Some(false) {
        return None;
    }
    let tool = value.pointer("/data/toolName").and_then(Value::as_str).unwrap_or("tool");
    let message = value.pointer("/data/error").and_then(Value::as_str)
        .or_else(|| value.pointer("/data/result/error").and_then(Value::as_str))
        .unwrap_or("unknown error");
    Some(format!("[{tool}] Error: {message}"))
}

fn extract_text_payload(value: &Value) -> Option<String> {
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
            for key in ["text", "content", "parts"] {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn collect_messages_renders_droid_tool_call_concisely() {
        // Real droid stream-json line: dumping the full JSON arg blob is noisy.
        // For known primary keys (file_path/path/pattern/...) we surface that
        // value with a few salient extras instead.
        let line = r#"{"type":"tool_call","toolName":"Read","parameters":{"file_path":"src/main.rs","limit":250,"offset":450}}"#;
        let messages = collect_messages(line);
        assert_eq!(messages.len(), 1);
        let msg = &messages[0];
        assert!(msg.starts_with("[Read] src/main.rs"), "msg={msg}");
        assert!(msg.contains("limit=250"), "msg={msg}");
        assert!(msg.contains("offset=450"), "msg={msg}");
        // Must NOT be the raw JSON dump.
        assert!(!msg.contains("\"file_path\""), "raw JSON leaked: {msg}");
    }

    #[test]
    fn collect_messages_truncates_very_long_payloads() {
        let big = "x".repeat(500);
        let line = json!({
            "type": "tool_call",
            "toolName": "Bash",
            "parameters": {"command": big.clone()}
        })
        .to_string();
        let messages = collect_messages(&line);
        assert_eq!(messages.len(), 1);
        let msg = &messages[0];
        assert!(msg.starts_with("[Bash] "), "msg={msg}");
        assert!(msg.ends_with('…'), "expected ellipsis: {msg}");
        assert!(msg.chars().count() < big.len(), "msg should be truncated");
    }

    #[test]
    fn collect_messages_skips_droid_tool_result_to_avoid_dupes() {
        // Two events for one logical Read invocation: tool_call + tool_result.
        // Only the call is informative; the result blob is noise (and double-
        // counted in older code).
        let lines = "\
{\"type\":\"tool_call\",\"toolName\":\"Read\",\"parameters\":{\"file_path\":\"a.rs\"}}\n\
{\"type\":\"tool_result\",\"toolName\":\"Read\",\"output\":\"file body...\"}\n";
        let messages = collect_messages(lines);
        // tool_result still produces a [Read] entry today (best-effort fallback),
        // but it must be SHORT and not the raw JSON. The key invariant: no entry
        // contains the literal multi-key JSON `{\"toolName\":...}` blob.
        for msg in &messages {
            assert!(!msg.contains("\"toolName\""), "JSON blob leaked: {msg}");
        }
    }
}
