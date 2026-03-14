// Cursor Agent CLI adapter: builds `cursor agent` commands, parses text output.
// Cursor Agent runs with --trust for autonomous operation and --workspace for dir.

use anyhow::Result;
use chrono::Local;
use serde_json::json;
use std::process::Command;

use super::truncate::truncate_text;
use super::RunOpts;
use crate::types::*;

pub struct CursorAgent;

impl super::Agent for CursorAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Cursor
    }

    fn streaming(&self) -> bool {
        true
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let mut cmd = Command::new("cursor");
        if opts.read_only {
            cmd.args([
                "agent",
                "-p",
                prompt,
                "--mode",
                "plan",
                "--output-format",
                "stream-json",
            ]);
        } else {
            cmd.args([
                "agent",
                "-p",
                prompt,
                "--trust",
                "--output-format",
                "stream-json",
            ]);
        }
        if let Some(ref dir) = opts.dir {
            cmd.args(["--workspace", dir]);
        }
        if let Some(ref model) = opts.model {
            cmd.args(["--model", model]);
        }
        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        let now = Local::now();

        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            return parse_json_event(task_id, &v, now);
        }

        let (kind, detail) = classify_line(trimmed);
        kind.map(|k| TaskEvent {
            task_id: task_id.clone(),
            timestamp: now,
            event_kind: k,
            detail: truncate_text(detail, 80),
            metadata: None,
        })
    }

    fn parse_completion(&self, _output: &str) -> CompletionInfo {
        CompletionInfo {
            tokens: None,
            status: TaskStatus::Done,
            model: None,
            cost_usd: None,
        }
    }
}

fn parse_json_event(
    task_id: &TaskId,
    v: &serde_json::Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let event_type = v.get("type").and_then(|value| value.as_str())?;
    let (event_kind, detail, metadata) = match event_type {
        "system" => {
            let subtype = v
                .get("subtype")
                .and_then(|value| value.as_str())
                .unwrap_or("system");
            let detail = v
                .get("model")
                .and_then(|value| value.as_str())
                .map(|model| format!("{subtype}: {model}"))
                .unwrap_or_else(|| subtype.to_string());
            (EventKind::Reasoning, detail, None)
        }
        "assistant" => {
            let detail = v
                .pointer("/message/content/0/text")
                .and_then(|value| value.as_str())?
                .to_string();
            (EventKind::Reasoning, detail, None)
        }
        "result" => {
            let input_tokens = v
                .pointer("/usage/inputTokens")
                .and_then(|value| value.as_i64())
                .unwrap_or(0);
            let output_tokens = v
                .pointer("/usage/outputTokens")
                .and_then(|value| value.as_i64())
                .unwrap_or(0);
            let cache_read_tokens = v
                .pointer("/usage/cacheReadTokens")
                .and_then(|value| value.as_i64())
                .unwrap_or(0);
            let total_tokens = input_tokens + output_tokens + cache_read_tokens;
            let detail = format!(
                "tokens: {} in + {} out = {} ({} cached)",
                input_tokens, output_tokens, total_tokens, cache_read_tokens
            );
            let metadata = Some(json!({
                "tokens": total_tokens,
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
            }));
            (EventKind::Completion, detail, metadata)
        }
        _ => return None,
    };

    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind,
        detail: truncate_text(&detail, 80),
        metadata,
    })
}

fn classify_line(line: &str) -> (Option<EventKind>, &str) {
    if line.contains("error[") || line.contains("FAILED") || line.starts_with("Error:") {
        (Some(EventKind::Error), line)
    } else if line.contains("test result:") || (line.contains("running") && line.contains("test")) {
        (Some(EventKind::Test), line)
    } else if line.contains("Compiling") || line.contains("Finished") {
        (Some(EventKind::Build), line)
    } else if line.contains("git commit") {
        (Some(EventKind::Commit), line)
    } else if line.starts_with("Writing") || line.starts_with("Creating") || line.contains("wrote")
    {
        (Some(EventKind::FileWrite), line)
    } else if line.starts_with("Reading") {
        (Some(EventKind::FileRead), line)
    } else if line.len() > 10 {
        (Some(EventKind::Reasoning), line)
    } else {
        (None, line)
    }
}

#[cfg(test)]
mod tests {
    use super::CursorAgent;
    use crate::agent::Agent;
    use crate::types::{EventKind, TaskId};

    #[test]
    fn parses_result_event_with_usage() {
        let agent = CursorAgent;
        let line = r#"{"type":"result","subtype":"success","duration_ms":8549,"result":"Hello!","usage":{"inputTokens":3,"outputTokens":5,"cacheReadTokens":12260,"cacheWriteTokens":2896}}"#;
        let event = agent
            .parse_event(&TaskId("t-result".to_string()), line)
            .unwrap();

        assert_eq!(event.event_kind, EventKind::Completion);
        assert_eq!(event.detail, "tokens: 3 in + 5 out = 12268 (12260 cached)");
        assert_eq!(
            event
                .metadata
                .as_ref()
                .and_then(|value| value.get("tokens"))
                .and_then(|value| value.as_i64()),
            Some(12268)
        );
        assert_eq!(
            event
                .metadata
                .as_ref()
                .and_then(|value| value.get("input_tokens"))
                .and_then(|value| value.as_i64()),
            Some(3)
        );
        assert_eq!(
            event
                .metadata
                .as_ref()
                .and_then(|value| value.get("output_tokens"))
                .and_then(|value| value.as_i64()),
            Some(5)
        );
    }

    #[test]
    fn parses_assistant_message() {
        let agent = CursorAgent;
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Hello!"}]}}"#;
        let event = agent
            .parse_event(&TaskId("t-assistant".to_string()), line)
            .unwrap();

        assert_eq!(event.event_kind, EventKind::Reasoning);
        assert_eq!(event.detail, "Hello!");
        assert!(event.metadata.is_none());
    }
}
