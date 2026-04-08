// GitHub Copilot CLI adapter: builds `copilot -p` commands and parses JSON event streams.
// Exports CopilotAgent for non-interactive runs plus minimal tool/error/completion handling.
// Deps: serde_json event parsing, std::process::Command, and shared truncate helpers.

use anyhow::Result;
use chrono::Local;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use super::RunOpts;
use super::truncate::truncate_text;
use crate::rate_limit;
use crate::types::*;

pub struct CopilotAgent;

impl super::Agent for CopilotAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Copilot
    }

    fn streaming(&self) -> bool {
        true
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let mut cmd = Command::new("copilot");
        let prompt = effective_prompt(prompt, opts);
        cmd.args([
            "-p",
            &prompt,
            "--output-format",
            "json",
            "--stream",
            "on",
            "--allow-all-tools",
            "--no-ask-user",
            "--silent",
        ]);
        if let Some(model) = opts.model.as_deref() {
            cmd.args(["--model", model]);
        }
        for dir in allowed_dirs(opts) {
            cmd.args(["--add-dir", &dir]);
        }
        if let Some(dir) = opts.dir.as_deref() {
            cmd.current_dir(dir);
        }
        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        let value: Value = serde_json::from_str(line).ok()?;
        let now = Local::now();
        let event_type = value.get("type").and_then(Value::as_str)?;
        match event_type {
            "assistant.message" => parse_assistant_message(task_id, &value, now),
            "tool.execution_start" => parse_tool_start(task_id, &value, now),
            "tool.execution_complete" => parse_tool_error(task_id, &value, now),
            "session.tools_updated" => parse_model_update(task_id, &value, now),
            "session.error" => parse_session_error(task_id, &value, now),
            "error" => parse_plain_error(task_id, &value, now),
            "result" => Some(TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: EventKind::Completion,
                detail: completion_detail(&value),
                metadata: completion_metadata(&value),
            }),
            _ => None,
        }
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

fn effective_prompt(prompt: &str, opts: &RunOpts) -> String {
    if !opts.read_only {
        return prompt.to_string();
    }
    if opts.result_file.is_some() {
        format!(
            "IMPORTANT: READ-ONLY MODE. Do NOT modify, create, or delete any files, EXCEPT the result file specified in this prompt. Only read, analyze, and write your findings to the designated result file.\n\n{prompt}"
        )
    } else {
        format!(
            "IMPORTANT: READ-ONLY MODE. Do NOT modify, create, or delete any files. Only read and analyze.\n\n{prompt}"
        )
    }
}

fn allowed_dirs(opts: &RunOpts) -> Vec<String> {
    let mut dirs = BTreeSet::new();
    if let Some(dir) = opts.dir.as_deref() {
        dirs.insert(dir.to_string());
    }
    for file in &opts.context_files {
        let path = Path::new(file);
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            dirs.insert(parent.to_string_lossy().into_owned());
        }
    }
    dirs.into_iter().collect()
}

fn parse_assistant_message(
    task_id: &TaskId,
    value: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let data = value.get("data")?;
    let content = data.get("content").and_then(Value::as_str)?;
    if content.is_empty() {
        return None;
    }
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Reasoning,
        detail: truncate_text(content, 80),
        metadata: data
            .get("outputTokens")
            .and_then(Value::as_i64)
            .map(|tokens| json!({ "output_tokens": tokens })),
    })
}

fn parse_tool_start(task_id: &TaskId, value: &Value, now: chrono::DateTime<Local>) -> Option<TaskEvent> {
    let data = value.get("data")?;
    let tool_name = data.get("toolName").and_then(Value::as_str).unwrap_or("tool");
    let detail = tool_start_detail(tool_name, data.get("arguments"))?;
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: classify_tool_kind(tool_name, data.get("arguments")),
        detail: truncate_text(&detail, 80),
        metadata: None,
    })
}

fn parse_tool_error(task_id: &TaskId, value: &Value, now: chrono::DateTime<Local>) -> Option<TaskEvent> {
    let data = value.get("data")?;
    if data.get("success").and_then(Value::as_bool) != Some(false) {
        return None;
    }
    let tool_name = data.get("toolName").and_then(Value::as_str).unwrap_or("tool");
    let detail = data
        .get("error")
        .and_then(Value::as_str)
        .or_else(|| data.pointer("/result/error").and_then(Value::as_str))
        .unwrap_or("unknown error");
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Error,
        detail: truncate_text(&format!("{tool_name}: {detail}"), 80),
        metadata: None,
    })
}

fn parse_model_update(task_id: &TaskId, value: &Value, now: chrono::DateTime<Local>) -> Option<TaskEvent> {
    let model = value.pointer("/data/model").and_then(Value::as_str)?;
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Milestone,
        detail: format!("model: {model}"),
        metadata: Some(json!({ "model": model })),
    })
}

fn parse_session_error(task_id: &TaskId, value: &Value, now: chrono::DateTime<Local>) -> Option<TaskEvent> {
    let data = value.get("data")?;
    if data.get("errorType").and_then(Value::as_str) == Some("persistence") {
        return None;
    }
    let detail = data.get("message").and_then(Value::as_str)?;
    mark_rate_limit_if_needed(detail);
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Error,
        detail: truncate_text(detail, 80),
        metadata: None,
    })
}

fn parse_plain_error(task_id: &TaskId, value: &Value, now: chrono::DateTime<Local>) -> Option<TaskEvent> {
    let detail = value
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/data/message").and_then(Value::as_str))
        .unwrap_or("unknown error");
    mark_rate_limit_if_needed(detail);
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Error,
        detail: truncate_text(detail, 80),
        metadata: None,
    })
}

fn mark_rate_limit_if_needed(detail: &str) {
    if rate_limit::is_rate_limit_error(detail) {
        rate_limit::mark_rate_limited(&AgentKind::Copilot, detail);
    }
}

fn classify_tool_kind(tool_name: &str, args: Option<&Value>) -> EventKind {
    if tool_name.eq_ignore_ascii_case("view") {
        return EventKind::FileRead;
    }
    if tool_name.eq_ignore_ascii_case("write") || tool_name.eq_ignore_ascii_case("edit") {
        return EventKind::FileWrite;
    }
    let command = args.and_then(|value| value.get("command")).and_then(Value::as_str);
    match command {
        Some(command) if command.contains(" test") || command.starts_with("test ") => EventKind::Test,
        Some(command) if command.contains(" build") || command.contains(" check") => EventKind::Build,
        _ => EventKind::ToolCall,
    }
}

fn tool_start_detail(tool_name: &str, args: Option<&Value>) -> Option<String> {
    let payload = args.and_then(|value| {
        value
            .get("path")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| value.get("command").and_then(Value::as_str).map(ToOwned::to_owned))
            .or_else(|| (!value.is_null()).then(|| value.to_string()))
    });
    payload.map(|payload| format!("{tool_name}: {payload}"))
}

fn completion_detail(value: &Value) -> String {
    let requests = value
        .pointer("/usage/premiumRequests")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if requests > 0 {
        format!("completed ({requests} premium request{})", if requests == 1 { "" } else { "s" })
    } else {
        "completed".to_string()
    }
}

fn completion_metadata(value: &Value) -> Option<serde_json::Value> {
    let requests = value.pointer("/usage/premiumRequests").and_then(Value::as_i64);
    let duration_ms = value.pointer("/usage/sessionDurationMs").and_then(Value::as_i64);
    let model = value.pointer("/data/model").and_then(Value::as_str);
    if requests.is_none() && duration_ms.is_none() && model.is_none() {
        return None;
    }
    let mut metadata = json!({});
    if let Some(requests) = requests {
        metadata["premium_requests"] = json!(requests);
    }
    if let Some(duration_ms) = duration_ms {
        metadata["session_duration_ms"] = json!(duration_ms);
    }
    if let Some(model) = model {
        metadata["model"] = json!(model);
    }
    Some(metadata)
}

#[cfg(test)]
#[path = "copilot_tests.rs"]
mod tests;
