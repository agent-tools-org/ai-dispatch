// Thin overlay that lets BYOK agents reuse the OpenCode adapter.
// Exports: OpenCodeOverlayAgent. Deps: super::opencode::OpenCodeAgent.

use anyhow::Result;
use std::process::Command;

use super::opencode::OpenCodeAgent;
use super::{Agent, RunOpts};
use crate::types::*;

pub struct OpenCodeOverlayAgent {
    pub id: String,
    pub display_name: String,
    pub forced_model: String,
    inner: OpenCodeAgent,
}

impl OpenCodeOverlayAgent {
    pub fn new(id: String, display_name: String, forced_model: String) -> Self {
        Self {
            id,
            display_name,
            forced_model,
            inner: OpenCodeAgent,
        }
    }
}

impl Agent for OpenCodeOverlayAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Custom
    }

    fn streaming(&self) -> bool {
        true
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        if opts.model.is_some() {
            return self.inner.build_command(prompt, opts);
        }
        let mut effective = opts.clone();
        effective.model = Some(self.forced_model.clone());
        self.inner.build_command(prompt, &effective)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        self.inner.parse_event(task_id, line)
    }

    fn parse_completion(&self, output: &str) -> CompletionInfo {
        self.inner.parse_completion(output)
    }

    fn needs_pty(&self) -> bool {
        self.inner.needs_pty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_opts() -> RunOpts {
        RunOpts {
            dir: None,
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: false,
            context_files: Vec::new(),
            session_id: None,
            env: None,
            env_forward: None,
        }
    }

    #[test]
    fn overlay_kind_is_custom() {
        let agent = OpenCodeOverlayAgent::new(
            "mimo".into(),
            "MiMo".into(),
            "mimo/mimo-v2.5-pro".into(),
        );
        assert_eq!(agent.kind(), AgentKind::Custom);
    }

    #[test]
    fn overlay_forces_model_when_unset() {
        let agent = OpenCodeOverlayAgent::new(
            "mimo".into(),
            "MiMo".into(),
            "mimo/mimo-v2.5-pro".into(),
        );
        let cmd = agent.build_command("hi", &base_opts()).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let i = args.iter().position(|a| a == "-m").expect("-m flag");
        assert_eq!(args.get(i + 1).map(String::as_str), Some("mimo/mimo-v2.5-pro"));
    }

    #[test]
    fn overlay_respects_caller_model_override() {
        let agent = OpenCodeOverlayAgent::new(
            "mimo".into(),
            "MiMo".into(),
            "mimo/mimo-v2.5-pro".into(),
        );
        let mut opts = base_opts();
        opts.model = Some("mimo/mimo-v2.5".into());
        let cmd = agent.build_command("hi", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let i = args.iter().position(|a| a == "-m").expect("-m flag");
        assert_eq!(args.get(i + 1).map(String::as_str), Some("mimo/mimo-v2.5"));
    }
}
