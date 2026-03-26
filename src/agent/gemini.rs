// Gemini CLI adapter: builds `gemini` commands, parses stream-json output.
// Gemini outputs stream-json events line-by-line during execution.

use anyhow::Result;
use chrono::Local;
use serde_json::json;
use std::process::Command;

use super::RunOpts;
use crate::types::*;

pub struct GeminiAgent;

impl super::Agent for GeminiAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Gemini
    }

    fn streaming(&self) -> bool {
        true
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let mut cmd = Command::new("gemini");
        cmd.args(["-o", "stream-json"]);
        if opts.read_only {
            cmd.args(["--approval-mode", "plan"]);
        } else {
            cmd.arg("-y");
        }
        if let Some(ref model) = opts.model {
            cmd.args(["-m", model]);
        }
        cmd.args(["-p", prompt]);
        if let Some(ref dir) = opts.dir {
            cmd.current_dir(dir);
        }
        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        let v: serde_json::Value = serde_json::from_str(trimmed).ok()?;
        let now = Local::now();
        parse_stream_event(task_id, &v, now)
    }

    fn parse_completion(&self, output: &str) -> CompletionInfo {
        let v: serde_json::Value = serde_json::from_str(output).unwrap_or_default();
        let tokens = extract_tokens(&v);
        let model = extract_model(&v);
        CompletionInfo {
            tokens,
            status: TaskStatus::Done,
            model,
            cost_usd: None,
            exit_code: None,
        }
    }
}

fn parse_stream_event(task_id: &TaskId, v: &serde_json::Value, now: chrono::DateTime<Local>) -> Option<TaskEvent> {
    let event_type = v.get("type")?.as_str()?;
    let (kind, detail, metadata) = match event_type {
        // "text" (pre-0.35) and "message" (0.35+) both carry assistant text
        "text" => {
            let content = v.get("content").and_then(|c| c.as_str())
                .or_else(|| v.get("text").and_then(|t| t.as_str()))?;
            (EventKind::Reasoning, content.to_string(), None)
        }
        "message" => {
            if v.get("role").and_then(|r| r.as_str()) != Some("assistant") {
                return None;
            }
            let content = v.get("content").and_then(|c| c.as_str())?;
            (EventKind::Reasoning, content.to_string(), None)
        }
        "tool_call" | "tool_use" => {
            let name = extract_tool_name(v).unwrap_or("unknown");
            let args = truncate(&extract_tool_arguments(v).unwrap_or_default(), 100);
            (EventKind::ToolCall, format!("{name}({args})"), None)
        }
        "tool_result" => {
            let name = extract_tool_name(v).unwrap_or("unknown");
            let output = v.get("output").and_then(|o| o.as_str()).unwrap_or("");
            let (k, d) = classify_tool_result(name, output);
            (k, d, None)
        }
        // "turn_complete" (pre-0.35) and "result" (0.35+) carry completion stats
        "turn_complete" | "result" => {
            let (tokens, model) = extract_completion_stats(v);
            let detail = match tokens {
                Some(t) => format!("completed with {t} tokens"),
                None => "completed".to_string(),
            };
            let meta = tokens.map(|t| json!({ "tokens": t, "model": model }));
            (EventKind::Completion, detail, meta)
        }
        _ => return None,
    };
    Some(TaskEvent { task_id: task_id.clone(), timestamp: now, event_kind: kind, detail, metadata })
}

fn extract_tool_name<'a>(v: &'a serde_json::Value) -> Option<&'a str> {
    v.get("tool_name")
        .and_then(|value| value.as_str())
        .or_else(|| v.get("name").and_then(|value| value.as_str()))
        .or_else(|| v.pointer("/functionCall/name").and_then(|value| value.as_str()))
        .or_else(|| v.get("function_call").and_then(|value| value.as_str()))
        .or_else(|| {
            v.get("function_call")
                .and_then(|value| value.get("name"))
                .and_then(|value| value.as_str())
        })
        .or_else(|| v.get("toolName").and_then(|value| value.as_str()))
        .or_else(|| v.get("tool").and_then(|value| value.as_str()))
        .or_else(|| {
            v.get("tool")
                .and_then(|value| value.get("name"))
                .and_then(|value| value.as_str())
        })
}

fn extract_tool_arguments(v: &serde_json::Value) -> Option<String> {
    [
        v.get("arguments"),
        v.pointer("/functionCall/args"),
        v.get("parameters"),
        v.get("input"),
    ]
    .into_iter()
    .flatten()
    .find_map(stringify_value)
}

fn stringify_value(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

fn classify_tool_result(name: &str, output: &str) -> (EventKind, String) {
    let lower_output = output.to_lowercase();
    let lower_name = name.to_lowercase();
    
    if lower_output.contains("error") || lower_output.contains("failed") || lower_output.contains("failure") {
        (EventKind::Error, format!("{}: {}", name, truncate(output, 80)))
    } else if lower_name.contains("test") || lower_output.contains("test") || lower_output.contains("passed") || lower_output.contains("failed") {
        (EventKind::Test, format!("{}: {}", name, truncate(output, 80)))
    } else if lower_name.contains("build") || lower_name.contains("compile") || lower_output.contains("compiled") || lower_output.contains("built") {
        (EventKind::Build, format!("{}: {}", name, truncate(output, 80)))
    } else {
        (EventKind::ToolCall, format!("{}: {}", name, truncate(output, 80)))
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let safe = s.floor_char_boundary(max_len.saturating_sub(3));
        format!("{}...", &s[..safe])
    }
}

/// Extract tokens + model from stats in both old and new gemini-cli formats.
/// Old (pre-0.35): stats.models = [{model, tokens:{total}}]
/// New (0.35+):    stats.total_tokens at top level, stats.models = {"name": {total_tokens}}
fn extract_completion_stats(v: &serde_json::Value) -> (Option<i64>, Option<String>) {
    let stats = match v.get("stats") {
        Some(s) => s,
        None => return (None, None),
    };
    // New format: top-level total_tokens
    if let Some(total) = stats.get("total_tokens").and_then(|t| t.as_i64()) {
        let model = stats
            .get("models")
            .and_then(|m| m.as_object())
            .and_then(|obj| obj.keys().next().cloned());
        return (Some(total), model);
    }
    // Old format: models array with tokens.total
    if let Some(arr) = stats.get("models").and_then(|m| m.as_array()) {
        let first = match arr.first() {
            Some(m) => m,
            None => return (None, None),
        };
        let tokens = first.pointer("/tokens/total").and_then(|t| t.as_i64());
        let model = first.get("model").and_then(|m| m.as_str()).map(|s| s.to_string());
        return (tokens, model);
    }
    (None, None)
}

pub fn extract_response(output: &str) -> Option<String> {
    // Collect all assistant message chunks (new format) and last text event (old format).
    // New gemini-cli streams delta messages; concatenate all assistant deltas.
    let mut assistant_chunks = Vec::new();
    let mut last_text_content: Option<String> = None;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) else { continue };
        let Some(event_type) = v.get("type").and_then(|t| t.as_str()) else { continue };
        match event_type {
            // New format (0.35+): assistant message deltas
            "message" if v.get("role").and_then(|r| r.as_str()) == Some("assistant") => {
                if let Some(content) = v.get("content").and_then(|c| c.as_str()) {
                    assistant_chunks.push(content.to_string());
                }
            }
            // Old format: text events
            "text" => {
                let content = v
                    .get("content")
                    .and_then(|c| c.as_str())
                    .or_else(|| v.get("text").and_then(|t| t.as_str()));
                if let Some(c) = content {
                    last_text_content = Some(c.to_string());
                }
            }
            _ => {}
        }
    }

    // Prefer new format (concatenated deltas) over old format (last text event)
    if !assistant_chunks.is_empty() {
        return Some(assistant_chunks.concat());
    }
    if let Some(text) = last_text_content {
        return Some(text);
    }

    // Fallback: try legacy single JSON format
    let v: serde_json::Value = serde_json::from_str(output).ok()?;
    if let Some(resp) = v.get("response").and_then(|r| r.as_str()) {
        return Some(resp.to_string());
    }
    if let Some(text) = v
        .pointer("/candidates/0/content/parts/0/text")
        .and_then(|t| t.as_str())
    {
        return Some(text.to_string());
    }
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    None
}

/// Extract total token count from gemini stats (both old and new formats).
fn extract_tokens(v: &serde_json::Value) -> Option<i64> {
    // New format: top-level stats.total_tokens
    if let Some(total) = v.pointer("/stats/total_tokens").and_then(|t| t.as_i64()) {
        return Some(total);
    }
    // Old format: stats.models array with tokens.total
    if let Some(arr) = v.pointer("/stats/models").and_then(|m| m.as_array()) {
        let total: i64 = arr
            .iter()
            .filter_map(|m| m.pointer("/tokens/total").and_then(|t| t.as_i64()))
            .sum();
        if total > 0 {
            return Some(total);
        }
    }
    // New format: models as object with total_tokens per model
    if let Some(obj) = v.pointer("/stats/models").and_then(|m| m.as_object()) {
        let total: i64 = obj
            .values()
            .filter_map(|m| m.get("total_tokens").and_then(|t| t.as_i64()))
            .sum();
        if total > 0 {
            return Some(total);
        }
    }
    v.pointer("/usageMetadata/totalTokenCount")
        .and_then(|t| t.as_i64())
}

fn extract_model(v: &serde_json::Value) -> Option<String> {
    for path in ["/modelVersion", "/model", "/stats/models/0/model"] {
        if let Some(m) = v.pointer(path).and_then(|v| v.as_str()) {
            return Some(m.to_string());
        }
    }
    // New format: stats.models is an object keyed by model name
    if let Some(obj) = v.pointer("/stats/models").and_then(|m| m.as_object()) {
        if let Some(name) = obj.keys().next() {
            return Some(name.clone());
        }
    }
    None
}

/// Create a completion event for gemini tasks
pub fn make_completion_event(task_id: &TaskId, info: &CompletionInfo) -> TaskEvent {
    let detail = match info.tokens {
        Some(t) => format!("completed with {} tokens", t),
        None => "completed".to_string(),
    };
    let metadata = info.tokens.map(|tokens| json!({ "tokens": tokens }));
    TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Completion,
        detail,
        metadata,
    }
}

#[cfg(test)]
#[path = "gemini_tests.rs"]
mod tests;
