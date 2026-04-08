// Claude Code CLI adapter: builds `claude` commands and delegates stream parsing.
// Exports ClaudeAgent for streaming runs plus prompt/context assembly helpers.
// Depends on claude_events parsing helpers and std::process::Command.

use anyhow::{Result, bail};
use std::path::Path;
use std::process::Command;

use super::RunOpts;
use super::claude_events::parse_event_line;
use crate::types::*;

pub struct ClaudeAgent;

impl super::Agent for ClaudeAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Claude
    }

    fn streaming(&self) -> bool {
        true
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let mut cmd = Command::new("claude");
        let prompt = build_prompt(prompt, &opts.context_files)?;
        cmd.args([
            "-p",
            &prompt,
            "--output-format",
            "stream-json",
            "--verbose",
            "--dangerously-skip-permissions",
        ]);
        if opts.read_only {
            let allowed_tools = if opts.result_file.is_some() {
                "Read,Glob,Grep,LS,Write"
            } else {
                "Read,Glob,Grep,LS"
            };
            cmd.args(["--allowedTools", allowed_tools]);
        }
        if let Some(ref model) = opts.model {
            cmd.args(["--model", model]);
        }
        if let Some(ref dir) = opts.dir {
            if !Path::new(dir).is_dir() {
                bail!("Workspace path does not exist: {dir}");
            }
            cmd.args(["--add-dir", dir]);
            cmd.current_dir(dir);
        }
        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        parse_event_line(task_id, line)
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

#[cfg(test)]
#[path = "claude_tests.rs"]
mod tests;
