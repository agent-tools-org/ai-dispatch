// Structured log-to-text extraction helpers for `aid show`.
// Exports: `collect_messages` used by CLI/TUI/web output rendering.
// Deps: serde_json::Value for agent log event parsing.

use serde_json::Value;

#[derive(Default)]
struct MessageCollector {
    messages: Vec<String>,
    streaming_message: String,
    replaceable_message: Option<String>,
    /// Terminal result text, used only when no assistant content was collected.
    pending_result_text: Option<String>,
}

pub(super) fn collect_messages(content: &str) -> Vec<String> {
    let mut collector = MessageCollector::default();
    for line in content.lines() {
        let cleaned = crate::runtime::watcher::strip_terminal_escapes(line);
        let Ok(value) = serde_json::from_str::<Value>(&cleaned) else {
            continue;
        };
        collector.collect(&value);
    }
    collector.finish()
}

impl MessageCollector {
    fn collect(&mut self, value: &Value) {
        let value = if value.get("type").and_then(|k| k.as_str()) == Some("event") {
            value.get("event").unwrap_or(value)
        } else {
            value
        };
        match value.get("type").and_then(|kind| kind.as_str()) {
            Some("item.completed") => self.push_message(completed_agent_message(value)),
            Some("message" | "message_end") => self.collect_message_event(value),
            Some("message_update") => self.collect_message_update_event(value),
            Some("assistant.message") => self.collect_copilot_message_event(value),
            Some("assistant.message_delta") => {
                self.append_streaming(copilot_delta_text(value));
            }
            Some("text_delta") => {
                self.append_streaming(text_delta_text(value));
            }
            Some("assistant") => self.append_streaming(assistant_event_text(value)),
            Some("text") => self.collect_text_event(value),
            Some("thinking" | "thinking_start" | "thinking_delta" | "thinking_end") => {}
            Some(
                "result" | "turn_complete" | "completion" | "done" | "step_finish" | "turn_end"
                | "run_end",
            ) => {
                self.collect_result_event(value);
            }
            // For any unrecognized event shape from any agent adapter (including commandcode
            // or custom agents), check if the payload contains a top-level `finalText` or `/result/text`.
            // For existing adapters (e.g. commandcode's `{"type":"result","finalText":"..."}` or
            // third-party wrappers with `result.text`), this extracts the final response text
            // even if the event envelope type is not explicitly enumerated in the match.
            _ => {
                if self.collect_detail_kind(value) {
                    return;
                }
                if let Some(text) = result_event_text(value) {
                    self.push_message(Some(text));
                }
            }
        }
    }

    fn collect_detail_event(&mut self, value: &Value, detail: fn(&Value) -> Option<String>) {
        self.flush_pending();
        if let Some(detail) = detail(value) {
            self.messages.push(detail);
        }
    }

    fn collect_result_event(&mut self, value: &Value) {
        self.flush_pending();
        // commandcode: always emit `finalText`. claude/qwen: defer top-level `result`
        // to finish() so it is only used when no assistant text was collected.
        if let Some(text) = final_text_field(value) {
            self.push_message(Some(text));
        } else if let Some(text) = duplicate_prone_result_text(value) {
            self.pending_result_text = Some(text);
        }
    }

    fn collect_message_update_event(&mut self, value: &Value) {
        let text = extract_text_payload(value);
        let Some(text) = text.filter(|text| !text.is_empty()) else {
            return;
        };
        if !self.streaming_message.is_empty() {
            if text == self.streaming_message || text.starts_with(&self.streaming_message) {
                self.streaming_message = text;
                return;
            }
            if self.streaming_message.starts_with(&text) {
                return;
            }
        }
        self.streaming_message = text;
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
            self.messages
                .push(std::mem::take(&mut self.streaming_message));
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
            self.messages
                .push(std::mem::take(&mut self.streaming_message));
        }
    }

    fn finish(mut self) -> Vec<String> {
        self.flush_pending();
        if self.messages.is_empty() {
            if let Some(text) = self.pending_result_text.take() {
                self.messages.push(text);
            }
        }
        self.messages
    }
}

#[path = "output_payload.rs"]
mod payload;
use payload::*;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn collect_messages_renders_droid_tool_call_concisely() {
        // Real droid stream-json line: dumping the full JSON arg blob is noisy.
        // For known primary keys (file_path/path/pattern/...) we surface that
        // value with a few salient extras instead.
        let line = "{\"type\":\"tool_call\",\"toolName\":\"Read\",\
            \"parameters\":{\"file_path\":\"src/main.rs\",\"limit\":250,\"offset\":450}}";
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

    #[test]
    fn collect_messages_preserves_utf8_after_stripping_ansi() {
        let line =
            "\x1b[1m{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"I’m using\"}\x1b[0m";

        assert_eq!(collect_messages(line), vec!["I’m using"]);
    }
}

impl MessageCollector {
    fn collect_detail_kind(&mut self, value: &Value) -> bool {
        match value.get("type").and_then(|kind| kind.as_str()) {
            Some("tool.execution_start") => {
                self.collect_detail_event(value, copilot_tool_start_message);
            }
            Some("tool.execution_complete") => {
                self.collect_detail_event(value, copilot_tool_error_message);
            }
            Some(
                "tool_use" | "tool_call" | "function_call" | "tool_result" | "tool_queued"
                | "tool_running" | "tool_completed",
            ) => {
                self.collect_detail_event(value, tool_event_message);
            }
            Some("error") => {
                self.collect_detail_event(value, error_event_message);
            }
            _ => return false,
        }
        true
    }
}
