// Response extraction for Antigravity assistant envelopes.
// Exports: extract_response. Deps: serde_json.

use serde_json::Value;

pub fn extract_response(output: &str) -> Option<String> {
    let mut buffer = ResponseBuffer::default();
    for line in output.lines() {
        if let Ok(value) = serde_json::from_str::<Value>(line.trim()) {
            buffer.accept(&value);
        }
    }
    buffer.flush();
    if !buffer.messages.is_empty() {
        return Some(buffer.messages.join("\n\n"));
    }
    let value: Value = serde_json::from_str(output).ok()?;
    value
        .get("response")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .pointer("/candidates/0/content/parts/0/text")
                .and_then(Value::as_str)
        })
        .or_else(|| value.as_str())
        .map(str::to_string)
}

#[derive(Default)]
struct ResponseBuffer {
    messages: Vec<String>,
    streaming_message: String,
    replaceable_text: Option<String>,
}

impl ResponseBuffer {
    fn accept(&mut self, value: &Value) {
        match value.get("type").and_then(Value::as_str) {
            Some("message") if value.get("role").and_then(Value::as_str) == Some("assistant") => {
                if let Some(content) = extract_text_payload(value.get("content")) {
                    if value.get("delta").and_then(Value::as_bool) == Some(true) {
                        self.flush_replaceable();
                        self.streaming_message.push_str(&content);
                    } else {
                        self.flush();
                        self.messages.push(content);
                    }
                }
            }
            Some("text") => {
                if let Some(content) =
                    extract_text_payload(value.get("content").or_else(|| value.get("text")))
                {
                    if !self.streaming_message.is_empty() {
                        self.messages
                            .push(std::mem::take(&mut self.streaming_message));
                    }
                    self.replaceable_text = Some(content);
                }
            }
            Some(
                "tool_call" | "tool_use" | "tool_result" | "result" | "turn_complete" | "error",
            ) => {
                self.flush();
            }
            _ => {}
        }
    }

    fn flush_replaceable(&mut self) {
        if let Some(text) = self.replaceable_text.take() {
            self.messages.push(text);
        }
    }

    fn flush(&mut self) {
        self.flush_replaceable();
        if !self.streaming_message.is_empty() {
            self.messages
                .push(std::mem::take(&mut self.streaming_message));
        }
    }
}

fn extract_text_payload(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let parts = items
                .iter()
                .filter_map(|item| extract_text_payload(Some(item)))
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.concat())
        }
        Value::Object(map) => {
            for key in ["text", "content", "parts"] {
                if let Some(text) = map
                    .get(key)
                    .and_then(|item| extract_text_payload(Some(item)))
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

#[cfg(test)]
mod tests {
    use super::extract_response;

    #[test]
    fn revisions_deltas_and_tool_boundaries_preserve_existing_response_order() {
        let output = concat!(
            "{\"type\":\"text\",\"content\":\"draft\"}\n",
            "{\"type\":\"text\",\"content\":\"revised\"}\n",
            "{\"type\":\"message\",\"role\":\"assistant\",\"delta\":true,\"content\":\"A\"}\n",
            "{\"type\":\"message\",\"role\":\"assistant\",\"delta\":true,\"content\":\"B\"}\n",
            "{\"type\":\"tool_call\"}\n",
            "{\"type\":\"message\",\"role\":\"assistant\",\"content\":{\"parts\":[{\"text\":\"C\"}]}}\n",
        );
        assert_eq!(
            extract_response(output).as_deref(),
            Some("revised\n\nAB\n\nC")
        );
    }

    #[test]
    fn legacy_response_shapes_remain_readable() {
        for output in [
            r#"{"response":"ok"}"#,
            r#"{"candidates":[{"content":{"parts":[{"text":"ok"}]}}]}"#,
            r#""ok""#,
        ] {
            assert_eq!(extract_response(output).as_deref(), Some("ok"));
        }
        assert_eq!(extract_response("plain text"), None);
    }
}
