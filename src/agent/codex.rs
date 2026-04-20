// Codex CLI adapter: builds `codex exec` commands and parses JSONL event streams.
// Exports CodexAgent for streaming runs plus helpers for tool and usage events.
// Depends on serde_json for metadata-rich completion events.

use anyhow::Result;
use chrono::Local;
use serde_json::{json, Map, Value};
use std::process::Command;
use std::sync::OnceLock;

use super::truncate::truncate_text;
use super::RunOpts;
use crate::rate_limit;
use crate::templates;
use crate::types::*;

/// Parsed codex CLI version (major, minor, patch).
/// Cached via OnceLock so `codex --version` runs at most once.
fn codex_version() -> (u32, u32, u32) {
    static VERSION: OnceLock<(u32, u32, u32)> = OnceLock::new();
    *VERSION.get_or_init(|| {
        Command::new("codex")
            .arg("--version")
            .output()
            .ok()
            .and_then(|out| {
                let text = String::from_utf8_lossy(&out.stdout);
                parse_semver(text.trim())
            })
            .unwrap_or((0, 0, 0))
    })
}

fn parse_semver(text: &str) -> Option<(u32, u32, u32)> {
    // "codex-cli 0.116.0" → "0.116.0"
    let ver = text.rsplit(' ').next()?;
    let mut parts = ver.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// Returns true if codex CLI supports the native `-m` / `--model` flag (≥ 0.116.0).
fn has_native_model_flag() -> bool {
    codex_version() >= (0, 116, 0)
}

pub struct CodexAgent;

impl super::Agent for CodexAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Codex
    }

    fn streaming(&self) -> bool {
        true
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let effective_prompt = if opts.read_only {
            if opts.result_file.is_some() {
                format!(
                    "IMPORTANT: READ-ONLY MODE. Do NOT modify, create, or delete any files, EXCEPT the result file specified in this prompt. Only read, analyze, and write your findings to the designated result file.\n\n{}",
                    prompt
                )
            } else {
                format!(
                    "IMPORTANT: READ-ONLY MODE. Do NOT modify, create, or delete any files. Only read and analyze.\n\n{}",
                    prompt
                )
            }
        } else {
            prompt.to_string()
        };
        let with_context = super::embed_context_in_prompt(&effective_prompt, &opts.context_files)?;
        let injected = templates::inject_codex_prompt(&with_context, None);
        let mut cmd = Command::new("codex");
        cmd.args(["exec", "--json", "--skip-git-repo-check", "--full-auto", &injected]);
        if let Some(ref model) = opts.model {
            if has_native_model_flag() {
                cmd.args(["-m", model]);
            } else {
                cmd.args(["-c", &format!("model=\"{model}\"")]);
            }
        }
        if let Some(ref output) = opts.output {
            cmd.args(["-o", output]);
        }
        if let Some(ref dir) = opts.dir {
            cmd.args(["-C", dir]);
            cmd.current_dir(dir);
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
            "thread.started" => parse_thread_started(task_id, &v, now),
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
            exit_code: None,
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
        "file_change" => parse_file_change_event(task_id, item, now),
        "error" => {
            let message = item.get("message").and_then(|m| m.as_str()).unwrap_or("");
            if message.is_empty() {
                return None;
            }
            if rate_limit::is_rate_limit_error(message) {
                rate_limit::mark_rate_limited(&AgentKind::Codex, message);
            }
            Some(TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: EventKind::Error,
                detail: truncate_text(message, 80),
                metadata: None,
            })
        }
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

    let cost_usd = v.get("cost_usd").and_then(|c| c.as_f64());
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
            cost_usd,
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

    if rate_limit::is_rate_limit_error(detail) {
        rate_limit::mark_rate_limited(&AgentKind::Codex, detail);
    }

    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Error,
        detail: truncate_text(detail, 80),
        metadata: None,
    })
}

fn parse_thread_started(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let thread_id = v.get("thread_id")?.as_str()?;
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Milestone,
        detail: format!("session {}", thread_id),
        metadata: Some(json!({ "agent_session_id": thread_id })),
    })
}

fn parse_file_change_event(
    task_id: &TaskId,
    item: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let changes = item.get("changes")?.as_array()?;
    let paths: Vec<&str> = changes
        .iter()
        .filter_map(|c| c.get("path").and_then(|p| p.as_str()))
        .collect();
    if paths.is_empty() {
        return None;
    }
    let detail = if paths.len() == 1 {
        truncate_text(paths[0], 80)
    } else {
        format!("{} files changed", paths.len())
    };
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::FileWrite,
        detail,
        metadata: Some(json!({ "files": paths })),
    })
}

fn completion_metadata(
    total_tokens: i64,
    input_tokens: i64,
    output_tokens: i64,
    cached_input_tokens: i64,
    model: Option<String>,
    cost_usd: Option<f64>,
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
    if let Some(cost) = cost_usd {
        map.insert("cost_usd".to_string(), json!(cost));
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
    use super::{parse_semver, CodexAgent};
    use crate::agent::{Agent, RunOpts};
    use crate::types::{EventKind, TaskId};

    #[test]
    fn semver_parsing() {
        assert_eq!(parse_semver("codex-cli 0.116.0"), Some((0, 116, 0)));
        assert_eq!(parse_semver("codex-cli 0.99.3"), Some((0, 99, 3)));
        assert_eq!(parse_semver("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("garbage"), None);
    }

    #[test]
    fn version_comparison_for_model_flag() {
        assert!((0, 116, 0) >= (0, 116, 0));
        assert!((0, 117, 0) >= (0, 116, 0));
        assert!((1, 0, 0) >= (0, 116, 0));
        assert!((0, 115, 9) < (0, 116, 0));
        assert!((0, 0, 0) < (0, 116, 0));
    }

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
    fn parses_thread_started_session_id() {
        let agent = CodexAgent;
        let line = r#"{"type":"thread.started","thread_id":"019d1efa-5aa6-7132-bdfa-71fb97e12438"}"#;
        let event = agent
            .parse_event(&TaskId("t-thread".to_string()), line)
            .unwrap();
        assert_eq!(event.event_kind, EventKind::Milestone);
        assert_eq!(
            event
                .metadata
                .unwrap()
                .get("agent_session_id")
                .and_then(|v| v.as_str()),
            Some("019d1efa-5aa6-7132-bdfa-71fb97e12438")
        );
    }

    #[test]
    fn parses_file_change_events() {
        let agent = CodexAgent;
        let line = r#"{"type":"item.completed","item":{"id":"item_5","type":"file_change","changes":[{"path":"/tmp/test.txt","kind":"update"}],"status":"completed"}}"#;
        let event = agent
            .parse_event(&TaskId("t-file".to_string()), line)
            .unwrap();
        assert_eq!(event.event_kind, EventKind::FileWrite);
        assert!(event.detail.contains("test.txt"));
    }

    #[test]
    fn parses_item_error_events() {
        let agent = CodexAgent;
        let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"error","message":"Model metadata for `o3` not found."}}"#;
        let event = agent
            .parse_event(&TaskId("t-err".to_string()), line)
            .unwrap();
        assert_eq!(event.event_kind, EventKind::Error);
        assert!(event.detail.contains("Model metadata"));
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

    #[test]
    fn build_command_includes_skip_git_repo_check() {
        let opts = RunOpts {
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
        };
        let cmd = CodexAgent.build_command("test prompt", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        assert!(args.contains(&"--skip-git-repo-check".to_string()));
    }

    #[test]
    fn build_command_read_only_uses_full_auto() {
        let opts = RunOpts {
            dir: None,
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: true,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = CodexAgent.build_command("analyze this code", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        assert!(args.contains(&"--full-auto".to_string()));
        assert!(!args.contains(&"-s".to_string()));
        assert!(!args.contains(&"read-only".to_string()));
    }

    #[test]
    fn build_command_read_only_prepends_readonly_prefix() {
        let opts = RunOpts {
            dir: None,
            output: None,
            result_file: Some("result.md".to_string()),
            model: None,
            budget: false,
            read_only: true,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = CodexAgent.build_command("analyze this code", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        let last_arg = args.last().expect("should have prompt as last arg");
        assert!(last_arg.contains("READ-ONLY MODE"));
        assert!(last_arg.starts_with("IMPORTANT: READ-ONLY MODE"));
        assert!(last_arg.contains("EXCEPT the result file specified in this prompt"));
        assert!(last_arg.contains("analyze this code"));
    }

    #[test]
    fn build_command_read_only_without_result_file_keeps_strict_prefix() {
        let opts = RunOpts {
            dir: None,
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: true,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = CodexAgent.build_command("analyze this code", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        let last_arg = args.last().expect("should have prompt as last arg");
        assert!(last_arg.contains("Do NOT modify, create, or delete any files. Only read and analyze."));
    }

    #[test]
    fn build_command_includes_context_files_in_prompt() {
        let opts = RunOpts {
            dir: None,
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: false,
            context_files: vec!["Cargo.toml".to_string()],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = CodexAgent.build_command("test prompt", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        let last_arg = args.last().expect("should have prompt as last arg");
        assert!(last_arg.contains("[Context File:"));
    }
}
