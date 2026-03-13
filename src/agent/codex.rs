// Codex CLI adapter: builds `codex exec` commands and parses JSONL event streams.
// Exports CodexAgent for streaming runs plus helpers for tool and usage events.
// Depends on serde_json for metadata-rich completion events.

use anyhow::Result;
use chrono::Local;
use serde_json::{Map, Value, json};
use std::process::Command;

use super::RunOpts;
use super::truncate::truncate_text;
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
            "item.started" | "item.completed" => parse_item_event(task_id, &v, now),
            "turn.completed" => parse_turn_completed(task_id, &v, now),
            "error" => parse_error_event(task_id, &v, now),
            _ => None,
        }
    }

    fn parse_completion(&self, _output: &str) -> CompletionInfo {
        // Codex is streaming — usage arrives in turn.completed events.
        CompletionInfo {
            tokens: None,
            status: TaskStatus::Done,
            model: None,
            cost_usd: None,
        }
    }
}

fn parse_item_event(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let event_type = v.get("type")?.as_str()?;
    let item = v.get("item")?;
    let item_type = item.get("type")?.as_str()?;

    match item_type {
        "agent_message" => {
            let text = item
                .get("text")
                .or_else(|| item.get("content"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            if text.is_empty() {
                return None;
            }
            Some(TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: EventKind::Reasoning,
                detail: truncate_text(text, 80),
                metadata: None,
            })
        }
        "command_execution" => parse_command_event(task_id, item, event_type, now),
        _ => None,
    }
}

fn parse_command_event(
    task_id: &TaskId,
    item: &Value,
    event_type: &str,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let command = item.get("command").and_then(|v| v.as_str()).unwrap_or("");
    if command.is_empty() {
        return None;
    }

    if event_type == "item.started" {
        return Some(TaskEvent {
            task_id: task_id.clone(),
            timestamp: now,
            event_kind: classify_command(command),
            detail: truncate_text(command, 80),
            metadata: Some(json!({ "command": command, "status": "in_progress" })),
        });
    }

    let exit_code = item.get("exit_code").and_then(|v| v.as_i64());
    if matches!(exit_code, Some(code) if code != 0) {
        return Some(TaskEvent {
            task_id: task_id.clone(),
            timestamp: now,
            event_kind: EventKind::Error,
            detail: format!(
                "command failed ({}) {}",
                exit_code.unwrap_or(-1),
                truncate_text(command, 60)
            ),
            metadata: Some(json!({ "command": command, "exit_code": exit_code })),
        });
    }

    let output = item
        .get("aggregated_output")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let event_kind = classify_output(output)?;
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind,
        detail: truncate_text(output, 80),
        metadata: Some(json!({ "command": command, "exit_code": exit_code })),
    })
}

fn parse_turn_completed(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let usage = v.get("usage")?;
    let input_tokens = usage
        .get("input_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let cached_input_tokens = usage
        .get("cached_input_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("output_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let total_tokens = input_tokens + output_tokens;
    let detail = if cached_input_tokens > 0 {
        format!(
            "tokens: {} in + {} out = {} ({} cached)",
            input_tokens, output_tokens, total_tokens, cached_input_tokens
        )
    } else {
        format!(
            "tokens: {} in + {} out = {}",
            input_tokens, output_tokens, total_tokens
        )
    };

    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Completion,
        detail,
        metadata: Some(completion_metadata(
            total_tokens,
            input_tokens,
            output_tokens,
            cached_input_tokens,
            extract_model(v),
        )),
    })
}

fn parse_error_event(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let detail = v
        .get("message")
        .or_else(|| v.pointer("/error/message"))
        .and_then(|value| value.as_str())
        .filter(|message| !message.is_empty())?;

    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Error,
        detail: truncate_text(detail, 80),
        metadata: None,
    })
}

fn completion_metadata(
    total_tokens: i64,
    input_tokens: i64,
    output_tokens: i64,
    cached_input_tokens: i64,
    model: Option<String>,
) -> Value {
    let mut map = Map::from_iter([
        ("tokens".to_string(), json!(total_tokens)),
        ("input_tokens".to_string(), json!(input_tokens)),
        ("output_tokens".to_string(), json!(output_tokens)),
        (
            "cached_input_tokens".to_string(),
            json!(cached_input_tokens),
        ),
    ]);
    if let Some(value) = model {
        map.insert("model".to_string(), json!(value));
    }
    Value::Object(map)
}

fn extract_model(v: &Value) -> Option<String> {
    [
        "/model",
        "/assistant/model",
        "/session/model",
        "/turn/model",
        "/usage/model",
        "/item/model",
    ]
    .iter()
    .find_map(|pointer| v.pointer(pointer).and_then(|value| value.as_str()))
    .map(ToOwned::to_owned)
}

fn classify_command(command: &str) -> EventKind {
    if command.contains("cargo test") || command.contains("npm test") {
        EventKind::Test
    } else if command.contains("cargo build") || command.contains("cargo check") {
        EventKind::Build
    } else if command.contains("git commit") {
        EventKind::Commit
    } else if command.contains("cargo fmt") || command.contains("prettier") {
        EventKind::Format
    } else if command.contains("cargo clippy") || command.contains("eslint") {
        EventKind::Lint
    } else {
        EventKind::ToolCall
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

#[cfg(test)]
mod tests {
    use super::CodexAgent;
    use crate::agent::Agent;
    use crate::types::{EventKind, TaskId};

    #[test]
    fn parses_agent_message_items() {
        let agent = CodexAgent;
        let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"Planning the next edit."}}"#;
        let event = agent
            .parse_event(&TaskId("t-msg".to_string()), line)
            .unwrap();
        assert_eq!(event.event_kind, EventKind::Reasoning);
        assert!(event.detail.contains("Planning"));
    }

    #[test]
    fn parses_turn_completed_usage_metadata() {
        let agent = CodexAgent;
        let line = r#"{"type":"turn.completed","usage":{"input_tokens":232452,"cached_input_tokens":211968,"output_tokens":5988}}"#;
        let event = agent
            .parse_event(&TaskId("t-usage".to_string()), line)
            .unwrap();
        assert_eq!(event.event_kind, EventKind::Completion);
        assert_eq!(
            event
                .metadata
                .unwrap()
                .get("tokens")
                .and_then(|v| v.as_i64()),
            Some(238440)
        );
    }
}
