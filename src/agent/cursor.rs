// Cursor Agent CLI adapter: builds `agent`/`cursor-agent` commands, parses stream-json output.
// Uses the standalone Cursor binary, preferring `agent` over the legacy alias.

use anyhow::Result;
use chrono::Local;
use serde_json::json;
use std::process::Command;

use super::truncate::truncate_text;
use super::RunOpts;
use crate::rate_limit;
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
        let binary = if super::env::which_exists("agent") { "agent" } else { "cursor-agent" };
        let mut cmd = Command::new(binary);
        let prompt_with_ctx = build_prompt(prompt, &opts.context_files)?;
        if opts.read_only {
            cmd.args([
                "-p",
                "--trust",
                &prompt_with_ctx,
                "--mode",
                "plan",
                "--output-format",
                "stream-json",
                "--stream-partial-output",
            ]);
        } else {
            cmd.args([
                "-p",
                &prompt_with_ctx,
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
        } else {
            cmd.args(["--model", "composer-2"]);
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

fn build_prompt(prompt: &str, context_files: &[String]) -> Result<String> {
    if context_files.is_empty() {
        return Ok(prompt.to_string());
    }
    let mut combined = prompt.to_string();
    for file in context_files {
        let contents = std::fs::read_to_string(file)?;
        combined.push_str("\n\n[Context File: ");
        combined.push_str(file);
        combined.push_str("]\n");
        combined.push_str(&contents);
    }
    Ok(combined)
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
        "error" => {
            let detail = v
                .get("message")
                .or_else(|| v.get("detail"))
                .or_else(|| v.get("error"))
                .and_then(|value| value.as_str())
                .unwrap_or("unknown error")
                .to_string();
            (EventKind::Error, detail, None)
        }
        _ => return None,
    };
    if event_kind == EventKind::Error || is_error_line(&detail) {
        maybe_mark_rate_limit(&detail);
    }

    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind,
        detail: truncate_text(&detail, 80),
        metadata,
    })
}

fn classify_line(line: &str) -> (Option<EventKind>, &str) {
    if is_error_line(line) {
        maybe_mark_rate_limit(line);
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

fn is_error_line(line: &str) -> bool {
    line.contains("error[") || line.contains("FAILED") || line.starts_with("Error:")
}

fn maybe_mark_rate_limit(detail: &str) {
    if rate_limit::is_rate_limit_error(detail) {
        rate_limit::mark_rate_limited(&AgentKind::Cursor, detail);
    }
}

#[cfg(test)]
mod tests {
    use super::CursorAgent;
    use crate::agent::RunOpts;
    use crate::agent::Agent;
    use crate::rate_limit;
    use crate::types::{EventKind, TaskId};
    use std::fs;
    use std::sync::{Mutex, OnceLock};

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
        let opts = run_opts();
        let cmd = agent.build_command("test prompt", &opts).unwrap();
        assert!(cmd.get_program() == "agent" || cmd.get_program() == "cursor-agent");
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args[0], "-p");
        assert!(args
            .windows(2)
            .any(|window| window[0] == "--model" && window[1] == "composer-2"));
    }

    #[test]
    fn build_command_embeds_context_files_in_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let context_file = dir.path().join("context.txt");
        fs::write(&context_file, "cursor context").unwrap();

        let agent = CursorAgent;
        let mut opts = run_opts();
        opts.context_files = vec![context_file.to_string_lossy().into_owned()];

        let cmd = agent.build_command("test prompt", &opts).unwrap();
        let args: Vec<_> = cmd.get_args().collect();
        let prompt = args[1].to_string_lossy();
        assert!(prompt.contains("test prompt"));
        assert!(prompt.contains("[Context File:"));
        assert!(prompt.contains("cursor context"));
    }

    #[test]
    fn read_only_build_command_adds_trust_and_context() {
        let dir = tempfile::tempdir().unwrap();
        let context_file = dir.path().join("readonly.txt");
        fs::write(&context_file, "readonly context").unwrap();

        let agent = CursorAgent;
        let mut opts = run_opts();
        opts.read_only = true;
        opts.context_files = vec![context_file.to_string_lossy().into_owned()];

        let cmd = agent.build_command("plan prompt", &opts).unwrap();
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args[0], "-p");
        assert_eq!(args[1], "--trust");
        let prompt = args[2].to_string_lossy();
        assert!(prompt.contains("plan prompt"));
        assert!(prompt.contains("readonly context"));
        assert!(args.windows(2).any(|window| window[0] == "--mode" && window[1] == "plan"));
    }

    #[test]
    fn parse_event_marks_plain_text_rate_limits() {
        let _guard = rate_limit_lock().lock().unwrap();
        let _ = rate_limit::clear_rate_limit(&crate::types::AgentKind::Cursor);
        let agent = CursorAgent;
        let line = "Error: rate limit exceeded, try again later";

        let event = agent
            .parse_event(&TaskId("t-rate-text".to_string()), line)
            .unwrap();

        assert_eq!(event.event_kind, EventKind::Error);
        assert_eq!(
            rate_limit::get_rate_limit_info(&crate::types::AgentKind::Cursor)
                .and_then(|info| info.message),
            Some(line.to_string())
        );
        let _ = rate_limit::clear_rate_limit(&crate::types::AgentKind::Cursor);
    }

    #[test]
    fn parse_event_marks_json_rate_limits() {
        let _guard = rate_limit_lock().lock().unwrap();
        let _ = rate_limit::clear_rate_limit(&crate::types::AgentKind::Cursor);
        let agent = CursorAgent;
        let line = r#"{"type":"error","message":"quota exceeded for this workspace"}"#;

        let event = agent
            .parse_event(&TaskId("t-rate-json".to_string()), line)
            .unwrap();

        assert_eq!(event.event_kind, EventKind::Error);
        assert_eq!(event.detail, "quota exceeded for this workspace");
        assert_eq!(
            rate_limit::get_rate_limit_info(&crate::types::AgentKind::Cursor)
                .and_then(|info| info.message),
            Some("quota exceeded for this workspace".to_string())
        );
        let _ = rate_limit::clear_rate_limit(&crate::types::AgentKind::Cursor);
    }

    fn rate_limit_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn run_opts() -> RunOpts {
        RunOpts {
            dir: None,
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: false,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        }
    }
}
