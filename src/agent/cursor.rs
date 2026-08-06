// Cursor Agent CLI adapter: builds `agent`/`cursor-agent` commands, parses stream-json output.
// Uses the standalone Cursor binary, preferring `agent` over the legacy alias.

use anyhow::Result;
use chrono::Local;
use serde_json::json;
use std::process::Command;
use std::sync::OnceLock;

use super::truncate::{capped_detail, capped_detail_with};
use super::RunOpts;
use crate::rate_limit;
use crate::types::*;

pub struct CursorAgent;

/// Cursor renamed `cursor-agent` to `agent`, so `agent` stays the preferred name — but it
/// is far too generic to take on faith. xAI's Grok Build CLI installs a binary called
/// exactly that, and handing Cursor's flags to it fails instantly with an unrelated
/// argument error that reads like a Cursor bug. Accept `agent` only when it says it is
/// Cursor's, and fall back to the unambiguous alias otherwise.
fn cursor_binary() -> &'static str {
    static RESOLVED: OnceLock<&'static str> = OnceLock::new();
    *RESOLVED.get_or_init(|| {
        if super::env::which_exists("agent") && identifies_as_cursor("agent") {
            return "agent";
        }
        "cursor-agent"
    })
}

fn identifies_as_cursor(binary: &str) -> bool {
    Command::new(binary)
        .arg("--help")
        .output()
        .map(|output| {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            help_mentions_cursor(&text)
        })
        .unwrap_or(false)
}

fn help_mentions_cursor(help: &str) -> bool {
    help.to_ascii_lowercase().contains("cursor")
}

impl super::Agent for CursorAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Cursor
    }

    fn streaming(&self) -> bool {
        true
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let mut cmd = Command::new(cursor_binary());
        let prompt_with_ctx = super::embed_context_in_prompt(prompt, &opts.context_files)?;
        // Cursor documents stream-json "assistant" events as deltas; only the terminal "result"
        // event is complete, so requesting --stream-partial-output just degrades logs into tokens.
        if opts.read_only {
            cmd.args([
                "-p",
                "--trust",
                &prompt_with_ctx,
                "--mode",
                "plan",
                "--output-format",
                "stream-json",
            ]);
        } else {
            cmd.args([
                "-p",
                &prompt_with_ctx,
                "--trust",
                "--force",
                "--output-format",
                "stream-json",
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
            // Cursor's own mid-tier model, kept as the default so an unspecified
            // run does not silently draw on the premium families it also serves
            // (Opus 5, GPT-5.6, Grok 4.5 are all reachable through this CLI).
            //
            // Model names rot: this said `composer-2` until 2026-08-05, by which
            // point `cursor-agent models` no longer listed it at all — only
            // composer-2.5 and composer-2.5-fast, with 2.5 marked "(current)".
            // Re-check against `cursor-agent models`.
            cmd.args(["--model", "composer-2.5"]);
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
        kind.map(|k| {
            let (detail, metadata) = capped_detail(detail);
            TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: k,
                detail,
                metadata,
            }
        })
    }

    fn parse_completion(&self, output: &str) -> CompletionInfo {
        // Real Cursor success ends with type:result + is_error:false; failures set is_error:true.
        super::stream_completion::status_from_result_jsonl(output)
    }

    fn served_models(&self) -> Result<Option<Vec<String>>> {
        let binary = cursor_binary();
        let mut cmd = Command::new(binary);
        cmd.arg("models");
        let output = super::model_validation::run_cmd_with_timeout(cmd, std::time::Duration::from_secs(2));
        let Some(text) = output else {
            return Ok(None);
        };
        let mut models = parse_cursor_models_output(&text);
        for alias in crate::types::ROUTER_ALIASES {
            if !models.iter().any(|m| m.eq_ignore_ascii_case(alias)) {
                models.push((*alias).to_string());
            }
        }
        Ok(Some(models))
    }
}

fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            let mut j = i + 2;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b';') {
                j += 1;
            }
            if j < bytes.len() && bytes[j].is_ascii_alphabetic() {
                i = j + 1;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn parse_cursor_models_output(output: &str) -> Vec<String> {
    let mut models = Vec::new();
    let cleaned = strip_ansi(output);
    for line in cleaned.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('<') {
            continue;
        }
        let name = trimmed.split_whitespace().next().unwrap_or("");
        if !name.is_empty() && !models.contains(&name.to_string()) {
            models.push(name.to_string());
        }
    }
    models
}

fn parse_json_event(
    task_id: &TaskId,
    v: &serde_json::Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let event_type = v.get("type").and_then(|value| value.as_str())?;
    let (event_kind, detail, metadata) = match event_type {
        "system" => parse_system_event(v),
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
        "tool_call" => parse_tool_call(v)?,
        "result" => parse_result_event(v),
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

    let (detail, metadata) = capped_detail_with(&detail, metadata);
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind,
        detail,
        metadata,
    })
}

fn parse_system_event(
    value: &serde_json::Value,
) -> (EventKind, String, Option<serde_json::Value>) {
    let subtype = value.get("subtype").and_then(|value| value.as_str()).unwrap_or("system");
    let model = value.get("model").and_then(|value| value.as_str());
    let session_id = value.get("session_id").and_then(|value| value.as_str());
    let detail = model
        .map(|model| format!("{subtype}: {model}"))
        .unwrap_or_else(|| subtype.to_string());
    let metadata = match (model, session_id) {
        (None, None) => None,
        _ => {
            let mut metadata = json!({});
            if let Some(model) = model { metadata["model"] = json!(model); }
            if let Some(session_id) = session_id {
                metadata["agent_session_id"] = json!(session_id);
            }
            Some(metadata)
        }
    };
    (EventKind::Reasoning, detail, metadata)
}

fn parse_result_event(
    value: &serde_json::Value,
) -> (EventKind, String, Option<serde_json::Value>) {
    let input = usage_i64(value, "inputTokens");
    let output = usage_i64(value, "outputTokens");
    let cached = usage_i64(value, "cacheReadTokens");
    let total = input + output + cached;
    let detail = format!("tokens: {input} in + {output} out = {total} ({cached} cached)");
    let mut metadata = json!({
        "tokens": total,
        "input_tokens": input,
        "output_tokens": output,
        "prompt_tokens": input,
    });
    if let Some(cost) = value.pointer("/usage/totalCostUSD").and_then(|value| value.as_f64()) {
        metadata["cost_usd"] = json!(cost);
    }
    (EventKind::Completion, detail, Some(metadata))
}

fn usage_i64(value: &serde_json::Value, key: &str) -> i64 {
    value.pointer(&format!("/usage/{key}")).and_then(|value| value.as_i64()).unwrap_or(0)
}

fn parse_tool_call(
    value: &serde_json::Value,
) -> Option<(EventKind, String, Option<serde_json::Value>)> {
    let subtype = value.get("subtype").and_then(|value| value.as_str()).unwrap_or("call");
    let calls = value.get("tool_call").and_then(|value| value.as_object())?;
    let (tool_name, tool_data) = calls
        .iter()
        .find(|(key, data)| key.ends_with("ToolCall") && data.is_object())?;
    let path = tool_path(tool_data);
    let argument = match tool_name.as_str() {
        "globToolCall" => tool_argument(tool_data, &["globPattern", "pattern"], "*").to_string(),
        "grepToolCall" => tool_argument(tool_data, &["pattern"], "?").to_string(),
        "shellToolCall" | "terminalToolCall" => {
            tool_argument(tool_data, &["command"], "?").to_string()
        }
        "writeToolCall" | "editToolCall" | "deleteToolCall" | "readToolCall" => {
            path.to_string()
        }
        _ => unknown_tool_key(tool_name, tool_data),
    };
    let action = match tool_name.as_str() {
        "writeToolCall" => "write",
        "editToolCall" => "edit",
        "deleteToolCall" => "delete",
        "readToolCall" => "read",
        "globToolCall" => "glob",
        "grepToolCall" => "grep",
        "shellToolCall" | "terminalToolCall" => "shell",
        _ => tool_name,
    };
    let kind = match tool_name.as_str() {
        "writeToolCall" | "editToolCall" | "deleteToolCall" => EventKind::FileWrite,
        "readToolCall" => EventKind::FileRead,
        _ => EventKind::ToolCall,
    };
    let metadata = match kind {
        EventKind::FileWrite | EventKind::FileRead => Some(json!({ "files": [&argument] })),
        EventKind::ToolCall => Some(json!({ "command": &argument })),
        _ => None,
    };
    Some((kind, format!("{subtype}: {action} {argument}"), metadata))
}

fn unknown_tool_key(tool_name: &str, value: &serde_json::Value) -> String {
    let arguments = value.get("args").unwrap_or(&serde_json::Value::Null);
    format!("{tool_name}:{arguments}")
}

fn tool_path(value: &serde_json::Value) -> &str {
    value
        .pointer("/args/path")
        .or_else(|| value.pointer("/args/filePath"))
        .and_then(|value| value.as_str())
        .unwrap_or("?")
}

fn tool_argument<'a>(value: &'a serde_json::Value, keys: &[&str], fallback: &'a str) -> &'a str {
    keys.iter()
        .find_map(|key| value.pointer(&format!("/args/{key}")).and_then(|value| value.as_str()))
        .unwrap_or(fallback)
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
#[path = "cursor_tests.rs"]
mod tests;
