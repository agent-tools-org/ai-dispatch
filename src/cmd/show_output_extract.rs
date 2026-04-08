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
            Some("assistant") => self.append_streaming(assistant_event_text(value)),
            Some("text") => self.collect_text_event(value),
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
    .find_map(stringify_payload);
    payload.map(|payload| format!("[{tool}] {payload}"))
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
