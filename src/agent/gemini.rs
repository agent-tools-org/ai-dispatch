// Gemini CLI adapter: builds `gemini` commands, parses stream-json output.
// Gemini outputs stream-json events line-by-line during execution.

use anyhow::Result;
use chrono::Local;
use serde_json::json;
use std::process::Command;

#[path = "gemini_support.rs"]
mod support;

use self::support::{
    classify_tool_result, extract_completion_stats, extract_error_detail, extract_model,
    extract_tokens, extract_tool_arguments, extract_tool_name,
};
use super::RunOpts;
use crate::rate_limit;
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
        // Gemini v0.36 has native sandboxing, but aid manages sandboxing outside the adapter.
        if opts.read_only {
            cmd.args(["--approval-mode", "plan"]);
        } else {
            cmd.arg("-y");
        }
        if let Some(ref model) = opts.model {
            cmd.args(["-m", model]);
        }
        for dir in support::gemini_include_directories(opts.dir.as_deref(), &opts.context_files) {
            cmd.args(["--include-directories", &dir]);
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
            let args = support::truncate(&extract_tool_arguments(v).unwrap_or_default(), 100);
            (EventKind::ToolCall, format!("{name}({args})"), None)
        }
        "tool_result" => {
            let name = extract_tool_name(v).unwrap_or("unknown");
            let output = v.get("output").and_then(|o| o.as_str()).unwrap_or("");
            let (k, d) = classify_tool_result(name, output);
            (k, d, None)
        }
        "error" => {
            let detail = extract_error_detail(v)?;
            if support::is_gemini_rate_limit_error(&detail) {
                rate_limit::mark_rate_limited(&AgentKind::Gemini, &detail);
            }
            (EventKind::Error, support::truncate(&detail, 80), None)
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
        kind if support::is_skill_or_hook_event(kind) => {
            (EventKind::Milestone, support::milestone_detail(kind, v), None)
        }
        _ => return None,
    };
    Some(TaskEvent { task_id: task_id.clone(), timestamp: now, event_kind: kind, detail, metadata })
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

#[cfg(test)]
#[path = "gemini_v036_tests.rs"]
mod v036_tests;
