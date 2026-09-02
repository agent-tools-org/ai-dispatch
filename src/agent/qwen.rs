// Qwen CLI adapter: builds `qwen` commands and parses stream-json output.
// Reuses Gemini support helpers for CLI flags, tool parsing, and truncation.

use anyhow::Result;
use chrono::Local;
use serde_json::json;
use std::process::Command;

use super::gemini_support as support;
use super::gemini_support::{
    classify_tool_result, extract_error_detail, extract_tool_arguments, extract_tool_name,
};
use super::RunOpts;
use crate::rate_limit;
use crate::types::*;

pub struct QwenAgent;

impl super::Agent for QwenAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Qwen
    }

    fn streaming(&self) -> bool {
        true
    }

    fn accepts_interactive_input(&self) -> bool {
        true
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let mut cmd = Command::new("qwen");
        cmd.args(["-o", "stream-json"]);
        let model = opts.model.clone()
            .or_else(crate::model_catalog::get_qwen_selected_model)
            .unwrap_or_else(|| "coder-model".to_string());
        cmd.args(["-m", &model]);
        if opts.sandbox { cmd.arg("--sandbox"); }
        if let Some(ref session_id) = opts.session_id { cmd.args(["-r", session_id]); }
        let prompt = super::embed_context_in_prompt(prompt, &opts.context_files)?;
        let allow_result_file = super::read_only::allow_result_file_write(opts);
        let effective_prompt = if allow_result_file { super::read_only::read_only_prompt(&prompt, opts) } else { prompt };
        if opts.read_only && !allow_result_file { cmd.args(["--approval-mode", "plan"]); } else { cmd.arg("-y"); }
        cmd.args(["-p", &effective_prompt]);
        if let Some(ref dir) = opts.dir { cmd.current_dir(dir); }
        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        let v: serde_json::Value = serde_json::from_str(trimmed).ok()?;
        parse_stream_event(task_id, &v, Local::now())
    }

    fn parse_completion(&self, output: &str) -> CompletionInfo {
        let mut tokens = None;
        let mut model = None;
        let mut status = TaskStatus::Done;

        for line in output.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if let Some(m) = extract_model(&v) {
                    model = Some(m);
                }
                if let Some(usage) = extract_usage(&v) {
                    tokens = Some(usage.total_tokens);
                }
                if let Some(event_type) = v.get("type").and_then(|t| t.as_str()) {
                    if event_type == "error" {
                        status = TaskStatus::Failed;
                    }
                    if event_type == "result" {
                        let terminal_api_error = v
                            .get("result")
                            .and_then(serde_json::Value::as_str)
                            .is_some_and(|text| text.trim_start().starts_with("[API Error:"));
                        if v.get("error").is_some()
                            || v.get("status").and_then(|s| s.as_str()) == Some("failed")
                            || terminal_api_error
                        {
                            status = TaskStatus::Failed;
                        }
                    }
                }
            }
        }

        CompletionInfo {
            tokens,
            status,
            model,
            cost_usd: None,
            exit_code: None,
        }
    }

    /// Note: This is a local config read from ~/.qwen/settings.json, not a served list
    /// returned by the Qwen CLI directly.
    fn served_models(&self) -> Result<Option<Vec<String>>> {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let settings_path = std::path::Path::new(&home).join(".qwen/settings.json");
        if let Ok(content) = std::fs::read_to_string(&settings_path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                let mut models = Vec::new();
                if let Some(name) = val.pointer("/model/name").and_then(|n| n.as_str()) {
                    models.push(name.to_string());
                }
                if let Some(providers) = val.get("modelProviders").and_then(|p| p.as_object()) {
                    for items in providers.values().filter_map(|v| v.as_array()) {
                        for item in items {
                            if let Some(id) = item.get("id").and_then(|i| i.as_str()) {
                                if !models.contains(&id.to_string()) {
                                    models.push(id.to_string());
                                }
                            }
                        }
                    }
                }
                if !models.is_empty() {
                    return Ok(Some(models));
                }
            }
        }
        Ok(None)
    }
}

fn parse_stream_event(task_id: &TaskId, v: &serde_json::Value, now: chrono::DateTime<Local>) -> Option<TaskEvent> {
    let event_type = v.get("type")?.as_str()?;
    let (kind, detail, metadata) = match event_type {
        "system" | "init" | "system/init" => {
            let subtype = v
                .get("subtype")
                .and_then(|value| value.as_str())
                .unwrap_or("init");
            let model = extract_model(v);
            let session_id = extract_session_id(v);
            let detail = model
                .as_deref()
                .map(|value| format!("{subtype}: {value}"))
                .unwrap_or_else(|| subtype.to_string());
            (EventKind::Reasoning, detail, base_metadata(model.as_deref(), session_id))
        }
        "assistant" => {
            let content = extract_assistant_text(v)?;
            let session_id = extract_session_id(v);
            (EventKind::Reasoning, content, base_metadata(None, session_id))
        }
        "text" => {
            let content = v.get("content").and_then(|c| c.as_str())
                .or_else(|| v.get("text").and_then(|t| t.as_str()))?;
            (EventKind::Reasoning, content.to_string(), None)
        }
        "message" => {
            if v.get("role").and_then(|r| r.as_str()) != Some("assistant") {
                return None;
            }
            let content = support::extract_text_payload(v.get("content"))?;
            (EventKind::Reasoning, content, None)
        }
        "tool_call" | "tool_use" => {
            let name = extract_tool_name(v).unwrap_or("unknown");
            let args = support::truncate(&extract_tool_arguments(v).unwrap_or_default(), 100);
            (EventKind::ToolCall, format!("{name}({args})"), None)
        }
        "tool_result" => {
            let name = extract_tool_name(v).unwrap_or("unknown");
            let output = v.get("output").and_then(|o| o.as_str()).unwrap_or("");
            let (kind, detail) = classify_tool_result(name, output);
            (kind, detail, None)
        }
        "error" => {
            let detail = extract_error_detail(v)?;
            if support::is_gemini_rate_limit_error(&detail) {
                rate_limit::mark_rate_limited(&AgentKind::Qwen, None, &detail);
            }
            (EventKind::Error, detail, None)
        }
        "result" | "turn_complete" => {
            let usage = extract_usage(v);
            let tokens = usage.map(|value| value.total_tokens);
            let model = extract_model(v);
            let detail = match tokens {
                Some(total) => format!("completed with {total} tokens"),
                None => "completed".to_string(),
            };
            let metadata = completion_metadata(usage, model.as_deref(), extract_session_id(v));
            (EventKind::Completion, detail, metadata)
        }
        kind if support::is_skill_or_hook_event(kind) => {
            (EventKind::Milestone, support::milestone_detail(kind, v), None)
        }
        _ => return None,
    };
    let (detail, metadata) = super::truncate::capped_detail_with(&detail, metadata);
    Some(TaskEvent { task_id: task_id.clone(), timestamp: now, event_kind: kind, detail, metadata })
}

fn completion_metadata(usage: Option<Usage>, model: Option<&str>, session_id: Option<&str>) -> Option<serde_json::Value> {
    let mut metadata = json!({});
    let mut has_fields = false;
    if let Some(usage) = usage {
        metadata["tokens"] = json!(usage.total_tokens);
        metadata["input_tokens"] = json!(usage.input_tokens);
        metadata["output_tokens"] = json!(usage.output_tokens);
        metadata["cache_read_input_tokens"] = json!(usage.cache_read_input_tokens);
        metadata["prompt_tokens"] = json!(usage.input_tokens);
        has_fields = true;
    }
    if let Some(model) = model {
        metadata["model"] = json!(model);
        has_fields = true;
    }
    if let Some(session_id) = session_id {
        metadata["agent_session_id"] = json!(session_id);
        has_fields = true;
    }
    has_fields.then_some(metadata)
}

fn base_metadata(model: Option<&str>, session_id: Option<&str>) -> Option<serde_json::Value> {
    match (model, session_id) {
        (None, None) => None,
        _ => {
            let mut metadata = json!({});
            if let Some(model) = model {
                metadata["model"] = json!(model);
            }
            if let Some(session_id) = session_id {
                metadata["agent_session_id"] = json!(session_id);
            }
            Some(metadata)
        }
    }
}

#[derive(Clone, Copy)]
struct Usage {
    input_tokens: i64,
    output_tokens: i64,
    cache_read_input_tokens: i64,
    total_tokens: i64,
}

fn extract_usage(value: &serde_json::Value) -> Option<Usage> {
    let input_tokens = value.pointer("/usage/input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
    let output_tokens = value.pointer("/usage/output_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
    let cache_read_input_tokens = value
        .pointer("/usage/cache_read_input_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let total_tokens = value
        .pointer("/usage/total_tokens")
        .and_then(|v| v.as_i64())
        .or_else(|| value.pointer("/stats/total_tokens").and_then(|v| v.as_i64()))
        .unwrap_or(input_tokens + output_tokens + cache_read_input_tokens);
    (total_tokens > 0).then_some(Usage {
        input_tokens,
        output_tokens,
        cache_read_input_tokens,
        total_tokens,
    })
}

fn extract_model(value: &serde_json::Value) -> Option<String> {
    value.pointer("/message/model")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("model").and_then(|v| v.as_str()))
        .map(ToOwned::to_owned)
}

fn extract_session_id(value: &serde_json::Value) -> Option<&str> {
    value.get("session_id")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("uuid").and_then(|v| v.as_str()))
}

fn extract_assistant_text(value: &serde_json::Value) -> Option<String> {
    support::extract_text_payload(
        value.get("message")
            .and_then(|message| message.get("content"))
            .or_else(|| value.get("content")),
    )
}

#[cfg(test)]
#[path = "qwen_tests.rs"]
mod tests;
