// OB1 agent adapter: Gemini CLI fork with identical stream-json protocol.
// Uses same event format, token extraction, and streaming as Gemini.

use anyhow::Result;
use std::process::Command;

use super::gemini::{extract_gemini_tokens, parse_gemini_event};
use super::RunOpts;
use crate::types::*;

pub struct Ob1Agent;

impl super::Agent for Ob1Agent {
    fn kind(&self) -> AgentKind {
        AgentKind::Ob1
    }

    fn streaming(&self) -> bool {
        true
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let mut cmd = Command::new("ob1");
        if opts.read_only {
            cmd.args(["-o", "stream-json", "--approval-mode", "plan", "-p", prompt]);
        } else {
            cmd.args(["-o", "stream-json", "--approval-mode", "yolo", "-p", prompt]);
        }
        if let Some(ref model) = opts.model {
            cmd.args(["-m", model]);
        }
        if let Some(ref dir) = opts.dir {
            cmd.current_dir(dir);
        }
        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        parse_gemini_event(task_id, line)
    }

    fn parse_completion(&self, output: &str) -> CompletionInfo {
        extract_gemini_tokens(output)
    }
}
