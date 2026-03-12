// Codex CLI adapter: builds `codex exec` commands, parses JSONL event stream.
// Codex outputs streaming JSONL with event types for tool calls, reasoning, and tokens.

use anyhow::Result;
use chrono::Local;
use std::process::Command;

use super::RunOpts;
use crate::templates;
use crate::types::*;

pub struct CodexAgent;

impl super::Agent for CodexAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Codex
    }

    fn streaming(&self) -> bool {
        true
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let injected = templates::inject_codex_prompt(prompt, None);
        let mut cmd = Command::new("codex");
        cmd.args(["exec", "--json", &injected]);
        if let Some(ref dir) = opts.dir {
            cmd.args(["-C", dir]);
        }
        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        let now = Local::now();

        // Check for NO_CHANGES_NEEDED in any text content
        if line.contains("NO_CHANGES_NEEDED") {
            return Some(TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: EventKind::NoOp,
                detail: extract_noop_reason(line),
                metadata: None,
            });
        }

        let event_type = v.get("type")?.as_str()?;
        match event_type {
            "response_item" => parse_response_item(task_id, &v, now),
            "event_msg" => parse_event_msg(task_id, &v, now),
            _ => None,
        }
    }

    fn parse_completion(&self, _output: &str) -> CompletionInfo {
        // Codex is streaming — completion is detected via parse_event
        CompletionInfo { tokens: None, status: TaskStatus::Done, model: None, cost_usd: None }
    }
}

fn parse_response_item(
    task_id: &TaskId,
    v: &serde_json::Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let payload = v.get("payload")?;
    let item_type = payload.get("type")?.as_str()?;

    match item_type {
        "function_call" => {
            let name = payload.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
            let args = payload.get("arguments")
                .and_then(|a| a.as_str())
                .map(|a| truncate_detail(a, 80))
                .unwrap_or_default();
            let detail = format!("{name}: {args}");
            let event_kind = classify_tool_call(name, &args);
            Some(TaskEvent { task_id: task_id.clone(), timestamp: now, event_kind, detail, metadata: None })
        }
        "function_call_output" => {
            let output = payload.get("output")
                .and_then(|o| o.as_str())
                .unwrap_or("");
            let event_kind = classify_output(output);
            if event_kind.is_some() {
                Some(TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: now,
                    event_kind: event_kind.unwrap(),
                    detail: truncate_detail(output, 80),
                    metadata: None,
                })
            } else {
                None
            }
        }
        "message" | "reasoning" => {
            let text = payload.get("text")
                .or_else(|| payload.get("content"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            if text.is_empty() { return None; }
            Some(TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: EventKind::Reasoning,
                detail: truncate_detail(text, 80),
                metadata: None,
            })
        }
        _ => None,
    }
}

fn parse_event_msg(
    task_id: &TaskId,
    v: &serde_json::Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let payload = v.get("payload")?;
    let msg_type = payload.get("type")?.as_str()?;

    match msg_type {
        "token_count" => {
            let input = payload.get("input_tokens").and_then(|t| t.as_i64()).unwrap_or(0);
            let output = payload.get("output_tokens").and_then(|t| t.as_i64()).unwrap_or(0);
            Some(TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: EventKind::Completion,
                detail: format!("tokens: {} in + {} out = {}", input, output, input + output),
                metadata: None,
            })
        }
        "task_started" | "user_message" => None, // Skip noise
        "agent_message" => {
            let text = payload.get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("");
            if text.is_empty() { return None; }
            Some(TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: EventKind::Reasoning,
                detail: truncate_detail(text, 80),
                metadata: None,
            })
        }
        _ => None,
    }
}

/// Classify tool calls into semantic event kinds
fn classify_tool_call(name: &str, args: &str) -> EventKind {
    match name {
        "exec_command" => {
            if args.contains("cargo test") || args.contains("npm test") {
                EventKind::Test
            } else if args.contains("cargo build") || args.contains("cargo check") {
                EventKind::Build
            } else if args.contains("git commit") {
                EventKind::Commit
            } else if args.contains("cargo fmt") || args.contains("prettier") {
                EventKind::Format
            } else if args.contains("cargo clippy") || args.contains("eslint") {
                EventKind::Lint
            } else {
                EventKind::ToolCall
            }
        }
        "write_file" | "create_file" | "patch_file" => EventKind::FileWrite,
        "read_file" => EventKind::FileRead,
        "web_search" => EventKind::WebSearch,
        _ => EventKind::ToolCall,
    }
}

/// Classify output lines for interesting events
fn classify_output(output: &str) -> Option<EventKind> {
    if output.contains("test result:") {
        Some(EventKind::Test)
    } else if output.contains("Finished") || output.contains("Compiling") {
        Some(EventKind::Build)
    } else if output.contains("error[") || output.contains("FAILED") {
        Some(EventKind::Error)
    } else {
        None
    }
}

fn extract_noop_reason(line: &str) -> String {
    if let Some(pos) = line.find("NO_CHANGES_NEEDED:") {
        let reason = &line[pos + 18..];
        format!("NO_CHANGES_NEEDED:{}", reason.trim().trim_matches('"'))
    } else {
        "NO_CHANGES_NEEDED".to_string()
    }
}

fn truncate_detail(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    if s.len() <= max {
        s
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

/// Extract token count from a completion event detail string
pub fn extract_tokens_from_detail(detail: &str) -> Option<i64> {
    // Format: "tokens: X in + Y out = Z"
    detail.rsplit("= ").next()
        .and_then(|s| s.trim().parse::<i64>().ok())
}
