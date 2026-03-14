// Codebuff agent adapter that delegates to the aid-codebuff CLI bridge.
// Exports: CodebuffAgent implementing the Agent trait for streaming runs.
// Deps: super::codex for parsing events and crate::types for metadata.

use anyhow::Result;
use std::process::Command;

use super::RunOpts;
use super::codex::CodexAgent;
use crate::types::*;

pub struct CodebuffAgent;

impl super::Agent for CodebuffAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Codebuff
    }

    fn streaming(&self) -> bool {
        true
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let mut cmd = Command::new("aid-codebuff");
        // SDK v0.10+ runs agent locally — needs extra heap for tokenizer + file scanning
        cmd.env("NODE_OPTIONS", "--max-old-space-size=8192");
        cmd.arg(prompt);
        if let Some(ref dir) = opts.dir {
            cmd.args(["--cwd", dir]);
        }
        if let Some(ref model) = opts.model {
            cmd.args(["--model", model]);
        }
        if opts.read_only {
            cmd.arg("--read-only");
        }
        if opts.budget {
            cmd.args(["--mode", "free"]);
        }
        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        CodexAgent.parse_event(task_id, line)
    }

    fn parse_completion(&self, output: &str) -> CompletionInfo {
        CodexAgent.parse_completion(output)
    }
}

#[cfg(test)]
mod tests {
    use super::CodebuffAgent;
    use crate::agent::Agent;
    use crate::types::{EventKind, TaskId};

    #[test]
    fn parses_codex_compatible_events() {
        let agent = CodebuffAgent;
        let line = r#"{"type":"item.completed","item":{"type":"agent_message","text":"Editing src/main.rs"}}"#;
        let event = agent
            .parse_event(&TaskId("t-cb".to_string()), line)
            .unwrap();
        assert_eq!(event.event_kind, EventKind::Reasoning);
        assert!(event.detail.contains("Editing"));
    }

    #[test]
    fn parses_turn_completed_usage() {
        let agent = CodebuffAgent;
        let line = r#"{"type":"turn.completed","usage":{"input_tokens":50000,"output_tokens":2000,"cached_input_tokens":0},"model":"claude-opus-4-6"}"#;
        let event = agent
            .parse_event(&TaskId("t-cb".to_string()), line)
            .unwrap();
        assert_eq!(event.event_kind, EventKind::Completion);
    }
}
