// Command Code CLI adapter: builds `commandcode -p` and parses NDJSON output.
// Exports CommandCodeAgent plus completion helpers for tests.

use anyhow::{bail, Result};
use chrono::Local;
use serde_json::Value;
use std::path::Path;
use std::process::Command;

use super::read_only::read_only_prompt;
use super::RunOpts;
use crate::types::*;

#[path = "commandcode_events.rs"]
mod events;
use events::{commandcode_result_failed, parse_line, usage_metadata};

pub struct CommandCodeAgent;

impl super::Agent for CommandCodeAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::CommandCode
    }

    fn streaming(&self) -> bool {
        true
    }

    fn accepts_interactive_input(&self) -> bool {
        true
    }

    fn accepts_idle_nudge(&self) -> bool {
        false
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let prompt = if opts.read_only {
            read_only_prompt(prompt, opts)
        } else {
            prompt.to_string()
        };
        let prompt = super::embed_context_in_prompt(&prompt, &opts.context_files)?;
        let mut cmd = Command::new("commandcode");
        cmd.args([
            "-p",
            &prompt,
            "--output-format",
            "json",
            "--skip-onboarding",
            "--no-auto-update",
        ]);
        if opts.read_only {
            cmd.args(["--permission-mode", "plan"]);
        } else {
            cmd.arg("--yolo");
        }
        if let Some(model) = opts.model.as_deref() {
            cmd.args(["-m", model]);
        }
        if let Some(session_id) = opts.session_id.as_deref() {
            cmd.args(["--session", session_id]);
        }
        if let Some(dir) = opts.dir.as_deref() {
            let path = Path::new(dir);
            if !path.is_dir() {
                bail!("commandcode working directory does not exist: {dir}");
            }
            cmd.args(["--add-dir", dir]);
            cmd.current_dir(dir);
        }
        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        let value: Value = serde_json::from_str(line.trim()).ok()?;
        parse_line(task_id, &value, Local::now())
    }

    fn parse_completion(&self, output: &str) -> CompletionInfo {
        parse_commandcode_completion(output)
    }
}


pub(crate) fn parse_commandcode_completion(output: &str) -> CompletionInfo {
    let mut info = CompletionInfo {
        tokens: None,
        status: TaskStatus::Failed,
        model: None,
        cost_usd: None,
        exit_code: None,
    };
    let mut saw_result = false;
    for line in output.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("event") => {
                let Some(event) = value.get("event") else {
                    continue;
                };
                match event.get("type").and_then(Value::as_str) {
                    Some("model_request_start") => {
                        if let Some(model) = event.get("model").and_then(Value::as_str) {
                            info.model = Some(model.to_string());
                        }
                    }
                    Some("turn_end") => {
                        if let Some(metadata) = event.get("usage").and_then(usage_metadata) {
                            info.tokens = metadata.get("tokens").and_then(Value::as_i64);
                        }
                    }
                    _ => {}
                }
            }
            Some("result") => {
                saw_result = true;
                if !commandcode_result_failed(&value) {
                    info.status = TaskStatus::Done;
                }
                if let Some(metadata) = value.get("usage").and_then(usage_metadata) {
                    info.tokens = metadata.get("tokens").and_then(Value::as_i64);
                }
            }
            Some("error") => info.status = TaskStatus::Failed,
            _ => {}
        }
    }
    if !saw_result {
        info.status = TaskStatus::Failed;
    }
    info
}

#[cfg(test)]
mod tests;
