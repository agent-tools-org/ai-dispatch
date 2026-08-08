// Oz (Warp) CLI adapter: builds `oz agent run` commands and parses streaming JSON events.
// Exports OzAgent for streaming runs.
// Depends on serde_json for event parsing.

use anyhow::Result;
use chrono::Local;
use std::process::Command;

use super::read_only::read_only_prompt;
use super::truncate::{capped_detail, capped_detail_with};
use super::RunOpts;
use crate::rate_limit;
use crate::types::*;

pub struct OzAgent;

fn parse_tool_call(v: &serde_json::Value) -> (EventKind, String, Option<serde_json::Value>) {
    let tool = v.get("tool").and_then(|t| t.as_str()).unwrap_or("tool");
    let title = v.get("title").and_then(|t| t.as_str()).unwrap_or(tool);
    let paths: Vec<&str> = v.get("file_paths").and_then(|p| p.as_array())
        .into_iter().flatten().filter_map(|p| p.as_str()).collect();
    let detail = match paths.is_empty() {
        true => title.to_string(),
        false => format!("{title}: {}", paths.join(", ")),
    };
    let metadata = (!paths.is_empty()).then(|| serde_json::json!({ "files": paths }));
    let kind = if tool == "edit_files" { EventKind::FileWrite } else { EventKind::ToolCall };
    let (detail, metadata) = capped_detail_with(&detail, metadata);
    (kind, detail, metadata)
}

impl super::Agent for OzAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Oz
    }

    fn streaming(&self) -> bool {
        true
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let prompt_with_ctx = super::embed_context_in_prompt(prompt, &opts.context_files)?;
        let effective_prompt = if opts.read_only {
            read_only_prompt(&prompt_with_ctx, opts)
        } else {
            prompt_with_ctx
        };
        let mut cmd = Command::new("oz");
        cmd.args(["agent", "run", "-p", &effective_prompt, "--output-format", "json"]);
        if let Some(ref dir) = opts.dir {
            cmd.args(["-C", dir]);
            cmd.current_dir(dir);
        }
        if let Some(ref model) = opts.model {
            cmd.args(["--model", model]);
        }
        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        let now = Local::now();
        let event_type = v.get("type")?.as_str()?;
        match event_type {
            "agent_reasoning" | "agent" => {
                let text = v.get("text").and_then(|t| t.as_str()).unwrap_or("");
                if text.is_empty() {
                    return None;
                }
                let (detail, metadata) = capped_detail(text);
                Some(TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: now,
                    event_kind: EventKind::Reasoning,
                    detail,
                    metadata,
                })
            }
            "tool_call" => {
                let (event_kind, detail, metadata) = parse_tool_call(&v);
                Some(TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: now,
                    event_kind,
                    detail,
                    metadata,
                })
            }
            "error" => {
                let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("unknown error");
                if rate_limit::is_rate_limit_error_for_agent(msg, &crate::types::AgentKind::Oz) {
                    rate_limit::mark_rate_limited(&crate::types::AgentKind::Oz, None, msg);
                }
                let (detail, metadata) = capped_detail(msg);
                Some(TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: now,
                    event_kind: EventKind::Error,
                    detail,
                    metadata,
                })
            }
            "tool_result" | "system" => None,
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "oz_tests.rs"]
mod tests;
