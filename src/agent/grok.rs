// Grok CLI adapter: builds `grok` commands and parses buffered JSON output.
// Probes the `grok` binary specifically — not the generic `agent` name.

use anyhow::{bail, Result};
use chrono::Local;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

use super::RunOpts;
use crate::rate_limit;
use crate::types::*;

pub struct GrokAgent;

impl super::Agent for GrokAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Grok
    }

    fn streaming(&self) -> bool {
        false
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let prompt_with_ctx = super::embed_context_in_prompt(prompt, &opts.context_files)?;
        let mut cmd = Command::new("grok");
        cmd.args(["-p", &prompt_with_ctx, "--output-format", "json"]);
        if opts.read_only {
            cmd.args(["--permission-mode", "plan"]);
        }
        if let Some(ref model) = opts.model {
            cmd.args(["--model", model]);
        }
        if let Some(ref session_id) = opts.session_id {
            cmd.args(["-r", session_id]);
        }
        if let Some(ref dir) = opts.dir {
            let path = Path::new(dir);
            if !path.is_dir() {
                bail!("Workspace path does not exist: {dir}");
            }
            cmd.args(["--cwd", dir]);
            cmd.current_dir(dir);
        }
        Ok(cmd)
    }

    fn parse_event(&self, _task_id: &TaskId, _line: &str) -> Option<TaskEvent> {
        None
    }

    fn parse_completion(&self, output: &str) -> CompletionInfo {
        parse_grok_completion(output)
    }
}

pub fn extract_response(output: &str) -> Option<String> {
    let value: Value = serde_json::from_str(output.trim()).ok()?;
    value
        .get("text")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

pub fn parse_grok_completion(output: &str) -> CompletionInfo {
    let trimmed = output.trim();
    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
        return failed_completion();
    };
    if value.get("type").and_then(Value::as_str) == Some("error") {
        if let Some(message) = value.get("message").and_then(Value::as_str) {
            maybe_mark_rate_limit(message);
        }
        return failed_completion();
    }
    let tokens = value
        .pointer("/usage/total_tokens")
        .and_then(Value::as_i64)
        .filter(|total| *total > 0);
    let model = extract_model_usage_key(&value);
    let cost_usd = value
        .get("total_cost_usd")
        .and_then(Value::as_f64)
        .or_else(|| model_usage_cost(&value));
    CompletionInfo {
        tokens,
        status: TaskStatus::Done,
        model,
        cost_usd,
        exit_code: None,
    }
}

fn failed_completion() -> CompletionInfo {
    CompletionInfo {
        tokens: None,
        status: TaskStatus::Failed,
        model: None,
        cost_usd: None,
        exit_code: None,
    }
}

fn extract_model_usage_key(value: &Value) -> Option<String> {
    value
        .get("modelUsage")
        .and_then(Value::as_object)
        .and_then(|usage| usage.keys().next())
        .cloned()
}

fn model_usage_cost(value: &Value) -> Option<f64> {
    let usage = value.get("modelUsage")?.as_object()?;
    usage.values().find_map(|entry| entry.get("costUSD").and_then(Value::as_f64))
}

fn maybe_mark_rate_limit(detail: &str) {
    if rate_limit::is_rate_limit_error(detail) {
        rate_limit::mark_rate_limited(&AgentKind::Grok, detail);
    }
}

pub fn make_completion_event(task_id: &TaskId, info: &CompletionInfo) -> TaskEvent {
    let detail = match info.tokens {
        Some(tokens) => format!("completed with {tokens} tokens"),
        None => "completed".to_string(),
    };
    let mut metadata = json!({});
    let mut has_fields = false;
    if let Some(tokens) = info.tokens {
        metadata["tokens"] = json!(tokens);
        has_fields = true;
    }
    if let Some(model) = info.model.as_deref() {
        metadata["model"] = json!(model);
        has_fields = true;
    }
    if let Some(cost_usd) = info.cost_usd {
        metadata["cost_usd"] = json!(cost_usd);
        has_fields = true;
    }
    TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Completion,
        detail,
        metadata: has_fields.then_some(metadata),
    }
}

#[cfg(test)]
#[path = "grok_tests.rs"]
mod tests;
