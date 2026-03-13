// Gemini CLI adapter: builds `gemini` commands, parses single JSON output.
// Gemini outputs a complete JSON blob at exit (not streaming JSONL).

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
        false
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let mut cmd = Command::new("gemini");
        if opts.read_only {
            cmd.args(["-o", "json", "--approval-mode", "plan", "-p", prompt]);
        } else {
            cmd.args(["-o", "json", "-y", "-p", prompt]);
        }
        cmd.stderr(std::process::Stdio::null());
        if let Some(ref output) = opts.output {
            let _ = output;
        }
        Ok(cmd)
    }

    fn parse_event(&self, _task_id: &TaskId, _line: &str) -> Option<TaskEvent> {
        // Gemini is not streaming — events aren't produced line-by-line
        None
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

/// Extract the response text from gemini JSON output
pub fn extract_response(output: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(output).ok()?;

    // Try common gemini output structures
    if let Some(resp) = v.get("response").and_then(|r| r.as_str()) {
        return Some(resp.to_string());
    }
    // Fallback: try .candidates[0].content.parts[0].text
    if let Some(text) = v
        .pointer("/candidates/0/content/parts/0/text")
        .and_then(|t| t.as_str())
    {
        return Some(text.to_string());
    }
    // If it's just a plain string response
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    None
}

/// Extract total token count from gemini stats
fn extract_tokens(v: &serde_json::Value) -> Option<i64> {
    // Try .stats.models[].tokens.total
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
    // Try .usageMetadata.totalTokenCount
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
}
