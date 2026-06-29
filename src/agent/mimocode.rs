// MiMo Code CLI adapter: thin wrapper over OpenCode-compatible JSON format.
// MiMo Code is an OpenCode-family CLI with identical event streaming.

use anyhow::Result;
use std::process::Command;

use super::opencode::{classify_text_line, extract_tokens_from_output, parse_json_event};
use super::RunOpts;
use crate::types::*;

pub struct MiMoCodeAgent;

impl super::Agent for MiMoCodeAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::MiMoCode
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
        let mut cmd = Command::new("mimo");
        cmd.arg("run");
        cmd.args(["--format", "json"]);
        cmd.arg("--thinking");
        cmd.arg("--dangerously-skip-permissions");
        if let Some(ref session_id) = opts.session_id {
            cmd.args(["--session", session_id]);
            cmd.arg("--continue");
            cmd.arg("--fork");
        }
        if opts.budget {
            cmd.args(["--variant", "minimal"]);
        }
        // MiMo's own CLI default (mimo-v2.5-pro-ultraspeed) is rejected by the
        // server (HTTP 400), so always pass an explicit, valid model. Fall back
        // to the auto-routing model when the caller hasn't chosen one.
        let model = opts.model.as_deref().unwrap_or("mimo/mimo-auto");
        cmd.args(["-m", model]);
        if let Some(ref dir) = opts.dir {
            cmd.args(["--dir", dir]);
            cmd.current_dir(dir);
        }
        for file in &opts.context_files {
            cmd.args(["-f", file]);
        }
        cmd.arg(&effective_prompt);
        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        let now = chrono::Local::now();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        let event = if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            parse_json_event(task_id, &v, now)
        } else {
            let (kind, detail) = classify_text_line(trimmed);
            kind.map(|k| TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: k,
                detail: super::truncate::truncate_text(detail, 80),
                metadata: None,
            })
        };
        if let Some(ref ev) = event
            && ev.event_kind == EventKind::Error
            && crate::rate_limit::is_rate_limit_error(&ev.detail)
        {
            crate::rate_limit::mark_rate_limited(&AgentKind::MiMoCode, &ev.detail);
        }
        event
    }

    fn parse_completion(&self, output: &str) -> CompletionInfo {
        let (tokens, cost_usd) = extract_tokens_from_output(output);
        CompletionInfo {
            tokens,
            status: TaskStatus::Done,
            model: None,
            cost_usd,
            exit_code: None,
        }
    }
}

#[cfg(test)]
mod tests;
