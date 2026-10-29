// Qwen CLI adapter: builds `qwen` commands and parses stream-json output.
// Uses Qwen-owned event helpers and provider-attributed quota classification.

use anyhow::Result;
use chrono::Local;

#[path = "qwen_events.rs"]
mod events;
use events::{extract_model, extract_usage, parse_stream_event};
use std::process::Command;

use super::RunOpts;
use crate::types::*;

pub struct QwenAgent;

impl super::Agent for QwenAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Qwen
    }

    fn default_model(&self) -> Option<String> {
        Some(
            crate::routing::model_catalog::get_qwen_selected_model()
                .unwrap_or_else(|| "coder-model".to_string()),
        )
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
        if let Some(model) = opts.model.clone().or_else(|| self.default_model()) {
            cmd.args(["-m", &model]);
        }
        if opts.sandbox {
            cmd.arg("--sandbox");
        }
        if let Some(ref session_id) = opts.session_id {
            cmd.args(["-r", session_id]);
        }
        let prompt = super::embed_context_in_prompt(prompt, &opts.context_files)?;
        let allow_result_file = super::read_only::allow_result_file_write(opts);
        let effective_prompt = if allow_result_file {
            super::read_only::read_only_prompt(&prompt, opts)
        } else {
            prompt
        };
        if opts.read_only && !allow_result_file {
            cmd.args(["--approval-mode", "plan"]);
        } else {
            cmd.arg("-y");
        }
        cmd.args(["-p", &effective_prompt]);
        if let Some(ref dir) = opts.dir {
            cmd.current_dir(dir);
        }
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
            tokens: tokens,
            model: model,
            ..CompletionInfo::new(status)
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

#[cfg(test)]
#[path = "qwen_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "qwen_quota_tests.rs"]
mod quota_tests;
