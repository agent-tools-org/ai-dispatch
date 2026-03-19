// Cursor Agent CLI adapter: builds `cursor-agent` commands, parses stream-json output.
// Uses the standalone cursor-agent binary (not the IDE CLI `cursor agent` subcommand).

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
        let mut cmd = Command::new("cursor-agent");
        if opts.read_only {
            cmd.args([
                "-p",
                prompt,
                "--mode",
                "plan",
                "--output-format",
                "stream-json",
                "--stream-partial-output",
            ]);
        } else {
            cmd.args([
                "-p",
                prompt,
                "--trust",
                "--force",
                "--output-format",
                "stream-json",
                "--stream-partial-output",
            ]);
        }
        if let Some(ref dir) = opts.dir {
            let path = std::path::Path::new(dir);
            if !path.is_dir() {
                anyhow::bail!("Workspace path does not exist: {dir}");
            }
            cmd.args(["--workspace", dir]);
            cmd.current_dir(dir);
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
            exit_code: None,
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
            let model = v.get("model").and_then(|value| value.as_str());
            let session_id = v.get("session_id").and_then(|value| value.as_str());
            let detail = model
                .map(|m| format!("{subtype}: {m}"))
                .unwrap_or_else(|| subtype.to_string());
            let metadata = match (model, session_id) {
                (None, None) => None,
                _ => {
                    let mut meta = json!({});
                    if let Some(m) = model { meta["model"] = json!(m); }
                    if let Some(sid) = session_id { meta["agent_session_id"] = json!(sid); }
                    Some(meta)
                }
            };
            (EventKind::Reasoning, detail, metadata)
        }
        "assistant" => {
            let detail = v
                .pointer("/message/content/0/text")
                .and_then(|value| value.as_str())?
                .to_string();
            (EventKind::Reasoning, detail, None)
        }
        "thinking" => {
            // Skip thinking deltas — they're tiny streaming fragments, not useful events
            return None;
        }
        "tool_call" => {
            let subtype = v
                .get("subtype")
                .and_then(|value| value.as_str())
                .unwrap_or("call");
            // Cursor uses tool-specific keys inside "tool_call" object:
            // e.g. {"tool_call": {"globToolCall": {...}}} or {"tool_call": {"writeToolCall": {...}}}
            let tc = v.get("tool_call").and_then(|value| value.as_object())?;
            let (tool_name, tool_data) = tc.iter().next()?;
            let path_from = |data: &serde_json::Value| -> String {
                data.pointer("/args/path")
                    .or_else(|| data.pointer("/args/filePath"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string()
            };
            let detail = match tool_name.as_str() {
                "writeToolCall" => format!("{subtype}: write {}", path_from(tool_data)),
                "readToolCall" => format!("{subtype}: read {}", path_from(tool_data)),
                "globToolCall" => {
                    let pattern = tool_data
                        .pointer("/args/globPattern")
                        .or_else(|| tool_data.pointer("/args/pattern"))
                        .and_then(|value| value.as_str())
                        .unwrap_or("*");
                    format!("{subtype}: glob {pattern}")
                }
                "shellToolCall" | "terminalToolCall" => {
                    let command = tool_data
                        .pointer("/args/command")
                        .and_then(|value| value.as_str())
                        .unwrap_or("?");
                    format!("{subtype}: shell {command}")
                }
                "grepToolCall" => {
                    let pattern = tool_data
                        .pointer("/args/pattern")
                        .and_then(|value| value.as_str())
                        .unwrap_or("?");
                    format!("{subtype}: grep {pattern}")
                }
                _ => format!("{subtype}: {tool_name}"),
            };
            let event_kind = match tool_name.as_str() {
                "writeToolCall" => EventKind::FileWrite,
                "readToolCall" => EventKind::FileRead,
                _ => EventKind::Reasoning,
            };
            (event_kind, detail, None)
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
            let cost_usd = v
                .pointer("/usage/totalCostUSD")
                .and_then(|value| value.as_f64());
            let mut meta = json!({
                "tokens": total_tokens,
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
                "prompt_tokens": input_tokens,
            });
            if let Some(cost) = cost_usd {
                meta["cost_usd"] = json!(cost);
            }
            (EventKind::Completion, detail, Some(meta))
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
    fn extracts_model_from_system_event() {
        let agent = CursorAgent;
        let line = r#"{"type":"system","subtype":"init","model":"composer-1.5"}"#;
        let event = agent
            .parse_event(&TaskId("t-sys".to_string()), line)
            .unwrap();
        assert_eq!(event.event_kind, EventKind::Reasoning);
        assert_eq!(event.detail, "init: composer-1.5");
        assert_eq!(
            event.metadata.as_ref().and_then(|v| v.get("model")).and_then(|v| v.as_str()),
            Some("composer-1.5")
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

    #[test]
    fn parses_tool_call_write() {
        let agent = CursorAgent;
        let line = r#"{"type":"tool_call","subtype":"started","tool_call":{"writeToolCall":{"args":{"filePath":"src/main.rs","content":"fn main() {}"}}}}"#;
        let event = agent
            .parse_event(&TaskId("t-tool".to_string()), line)
            .unwrap();
        assert_eq!(event.event_kind, EventKind::FileWrite);
        assert_eq!(event.detail, "started: write src/main.rs");
    }

    #[test]
    fn parses_tool_call_glob() {
        let agent = CursorAgent;
        let line = r#"{"type":"tool_call","subtype":"started","tool_call":{"globToolCall":{"args":{"globPattern":"**/*.rs","targetDirectory":"src/"}}}}"#;
        let event = agent
            .parse_event(&TaskId("t-tool".to_string()), line)
            .unwrap();
        assert_eq!(event.event_kind, EventKind::Reasoning);
        assert_eq!(event.detail, "started: glob **/*.rs");
    }

    #[test]
    fn skips_all_thinking_deltas() {
        let agent = CursorAgent;
        // All thinking events should be skipped, including non-empty ones
        let line = r#"{"type":"thinking","subtype":"delta","text":"analyzing the code"}"#;
        assert!(agent
            .parse_event(&TaskId("t-think".to_string()), line)
            .is_none());
    }

    #[test]
    fn uses_cursor_agent_binary() {
        let agent = CursorAgent;
        let opts = crate::agent::RunOpts {
            dir: None,
            output: None,
            model: None,
            budget: false,
            read_only: false,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = agent.build_command("test prompt", &opts).unwrap();
        assert_eq!(cmd.get_program(), "cursor-agent");
        // Should NOT have "agent" as first arg (no longer a subcommand)
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args[0], "-p");
    }
}
