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
        if opts.read_only {
            cmd.args(["-o", "stream-json", "--approval-mode", "plan", "-p", prompt]);
        } else {
            cmd.args(["-o", "stream-json", "--approval-mode", "yolo", "-p", prompt]);
        }
        if let Some(ref model) = opts.model {
            cmd.args(["-m", model]);
        }
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
        }
    }
}

fn parse_stream_event(task_id: &TaskId, v: &serde_json::Value, now: chrono::DateTime<Local>) -> Option<TaskEvent> {
    let event_type = v.get("type")?.as_str()?;
    match event_type {
        "text" => {
            let content = v
                .get("content")
                .and_then(|c| c.as_str())
                .or_else(|| v.get("text").and_then(|t| t.as_str()))?;
            Some(TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: EventKind::Reasoning,
                detail: content.to_string(),
                metadata: None,
            })
        }
        "tool_call" => {
            let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
            let args = v.get("arguments").and_then(|a| a.as_str()).unwrap_or("");
            let truncated_args = if args.len() > 100 {
                format!("{}...", &args[..100])
            } else {
                args.to_string()
            };
            Some(TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: EventKind::ToolCall,
                detail: format!("{}({})", name, truncated_args),
                metadata: None,
            })
        }
        "tool_result" => {
            let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
            let output = v.get("output").and_then(|o| o.as_str()).unwrap_or("");
            let (kind, detail) = classify_tool_result(name, output);
            Some(TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: kind,
                detail,
                metadata: None,
            })
        }
        "turn_complete" => {
            let (tokens, model) = extract_turn_complete_stats(v);
            let detail = match tokens {
                Some(t) => format!("completed with {} tokens", t),
                None => "completed".to_string(),
            };
            let metadata = tokens.map(|t| json!({ "tokens": t, "model": model }));
            Some(TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: EventKind::Completion,
                detail,
                metadata,
            })
        }
        _ => None,
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
        format!("{}...", &s[..max_len])
    }
}

fn extract_turn_complete_stats(v: &serde_json::Value) -> (Option<i64>, Option<String>) {
    let models = match v.pointer("/stats/models").and_then(|m| m.as_array()) {
        Some(arr) => arr,
        None => return (None, None),
    };
    let first_model = match models.first() {
        Some(m) => m,
        None => return (None, None),
    };
    let tokens = first_model
        .pointer("/tokens/total")
        .and_then(|t| t.as_i64());
    let model_name = first_model
        .get("model")
        .and_then(|m| m.as_str())
        .map(|s| s.to_string());
    (tokens, model_name)
}

pub fn extract_response(output: &str) -> Option<String> {
    let lines: Vec<&str> = output.lines().collect();
    
    // Try stream-json format first: find the last "text" event
    for line in lines.iter().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if v.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(content) = v.get("content").and_then(|c| c.as_str()) {
                    return Some(content.to_string());
                }
                if let Some(text) = v.get("text").and_then(|t| t.as_str()) {
                    return Some(text.to_string());
                }
            }
        }
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

/// Extract total token count from gemini stats
fn extract_tokens(v: &serde_json::Value) -> Option<i64> {
    if let Some(models) = v.pointer("/stats/models")
        && let Some(arr) = models.as_array()
    {
        let total: i64 = arr
            .iter()
            .filter_map(|m| m.pointer("/tokens/total").and_then(|t| t.as_i64()))
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
mod tests {
    use super::*;

    #[test]
    fn test_extract_model() {
        let json = serde_json::json!({
            "modelVersion": "gemini-2.5-pro",
            "response": "test"
        });
        assert_eq!(extract_model(&json), Some("gemini-2.5-pro".to_string()));

        let json2 = serde_json::json!({
            "stats": {
                "models": [{"model": "gemini-1.5-flash"}]
            }
        });
        assert_eq!(extract_model(&json2), Some("gemini-1.5-flash".to_string()));

        let json3 = serde_json::json!({ "response": "test" });
        assert_eq!(extract_model(&json3), None);
    }

    #[test]
    fn parses_text_event() {
        let task_id = TaskId::generate();
        let json = serde_json::json!({
            "type": "text",
            "content": "Hello world"
        });
        let event = parse_stream_event(&task_id, &json, Local::now()).unwrap();
        assert_eq!(event.task_id, task_id);
        assert_eq!(event.event_kind, EventKind::Reasoning);
        assert_eq!(event.detail, "Hello world");
    }

    #[test]
    fn parses_turn_complete_with_tokens() {
        let task_id = TaskId::generate();
        let json = serde_json::json!({
            "type": "turn_complete",
            "stats": {
                "models": [{
                    "model": "gemini-2.5-pro",
                    "tokens": {
                        "total": 1234,
                        "input": 500,
                        "output": 734
                    }
                }]
            }
        });
        let event = parse_stream_event(&task_id, &json, Local::now()).unwrap();
        assert_eq!(event.task_id, task_id);
        assert_eq!(event.event_kind, EventKind::Completion);
        assert!(event.detail.contains("1234"));
        let metadata = event.metadata.unwrap();
        assert_eq!(metadata["tokens"], 1234);
        assert_eq!(metadata["model"], "gemini-2.5-pro");
    }

    #[test]
    fn extract_response_from_stream_json() {
        let output = r#"{"type":"text","content":"First line"}
{"type":"text","content":"Second line"}
{"type":"turn_complete"}"#;
        let result = extract_response(output);
        assert_eq!(result, Some("Second line".to_string()));
    }

    #[test]
    fn parses_tool_call_event() {
        let task_id = TaskId::generate();
        let json = serde_json::json!({
            "type": "tool_call",
            "name": "Read",
            "arguments": "{\"file\": \"test.rs\"}"
        });
        let event = parse_stream_event(&task_id, &json, Local::now()).unwrap();
        assert_eq!(event.event_kind, EventKind::ToolCall);
        assert!(event.detail.starts_with("Read("));
    }

    #[test]
    fn parses_tool_result_test_event() {
        let task_id = TaskId::generate();
        let json = serde_json::json!({
            "type": "tool_result",
            "name": "run_tests",
            "output": "Tests passed successfully"
        });
        let event = parse_stream_event(&task_id, &json, Local::now()).unwrap();
        assert_eq!(event.event_kind, EventKind::Test);
    }
}