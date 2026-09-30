// Declarative overlay for CLIs that speak OpenCode-compatible JSONL.
// Exports OpenCodeOverlayAgent and OpenCodeOverlaySpec.
// Depends on opencode parsing helpers and shared read-only prompt handling.

use anyhow::Result;
use chrono::Local;
use std::process::Command;

use super::opencode::{classify_text_line, extract_tokens_from_output, parse_json_event};
use super::read_only::read_only_prompt;
use super::{Agent, RunOpts};
use crate::types::*;

#[derive(Debug, Clone)]
pub(crate) struct OpenCodeOverlaySpec {
    pub id: String,
    pub display_name: String,
    pub reported_kind: AgentKind,
    pub binary: String,
    pub extra_args: Vec<String>,
    pub default_model: Option<String>,
    pub interactive_input: bool,
    pub rate_limit_kind: AgentKind,
    pub allow_external_directories: bool,
    pub probe_served_models: bool,
}

pub struct OpenCodeOverlayAgent {
    spec: OpenCodeOverlaySpec,
}

impl OpenCodeOverlayAgent {
    pub fn new(id: String, display_name: String, forced_model: String) -> Self {
        Self::from_spec(OpenCodeOverlaySpec {
            id,
            display_name,
            reported_kind: AgentKind::Custom,
            binary: "opencode".to_string(),
            extra_args: Vec::new(),
            default_model: Some(forced_model),
            interactive_input: true,
            rate_limit_kind: AgentKind::OpenCode,
            allow_external_directories: true,
            probe_served_models: false,
        })
    }

    pub(crate) fn from_spec(spec: OpenCodeOverlaySpec) -> Self {
        Self {
            spec,
        }
    }
}

impl Agent for OpenCodeOverlayAgent {
    fn kind(&self) -> AgentKind {
        self.spec.reported_kind
    }

    fn default_model(&self) -> Option<String> {
        self.spec.default_model.clone()
    }

    fn rate_limit_name(&self) -> Option<&str> {
        if self.spec.reported_kind == AgentKind::Custom {
            Some(self.spec.id.as_str())
        } else {
            None
        }
    }

    fn streaming(&self) -> bool {
        true
    }

    fn accepts_interactive_input(&self) -> bool {
        self.spec.interactive_input
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        if opts.read_only && matches!(self.spec.reported_kind, AgentKind::OpenCode | AgentKind::Custom) {
            aid_warn!("[aid] ⚠OpenCode read-only is prompt-level only, not enforced. Use --worktree for isolation.");
        }
        let effective_prompt = if opts.read_only {
            read_only_prompt(prompt, opts)
        } else {
            prompt.to_string()
        };
        let mut cmd = Command::new(&self.spec.binary);
        cmd.arg("run");
        cmd.args(&self.spec.extra_args);
        cmd.args(["--format", "json"]);
        cmd.arg("--thinking");
        if self.spec.allow_external_directories {
            cmd.env(
                "OPENCODE_CONFIG_CONTENT",
                r#"{"agent":{"build":{"permission":{"external_directory":"allow"}}}}"#,
            );
        }
        if let Some(ref session_id) = opts.session_id {
            cmd.args(["--session", session_id]);
            cmd.arg("--continue");
            cmd.arg("--fork");
        }
        if opts.budget {
            cmd.args(["--variant", "minimal"]);
        }
        let model = opts.model.as_deref().or(self.spec.default_model.as_deref());
        if let Some(model) = model {
            cmd.args(["-m", model]);
        }
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
        let now = Local::now();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            return parse_json_event(
                self.spec.rate_limit_kind,
                self.spec.reported_kind,
                self.rate_limit_name(),
                task_id,
                &v,
                now,
            );
        }
        let (kind, detail) = classify_text_line(trimmed);
        kind.map(|event_kind| {
            let (detail, metadata) = super::truncate::capped_detail(detail);
            TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind,
                detail,
                metadata,
            }
        })
    }

    fn parse_completion(&self, output: &str) -> CompletionInfo {
        let (tokens, cost_usd) = extract_tokens_from_output(output);
        let mut info = super::stream_completion::status_from_error_type_jsonl(output);
        info.tokens = tokens;
        info.cost_usd = cost_usd;
        info
    }

    fn needs_pty(&self) -> bool {
        true
    }

    fn served_models(&self) -> Result<Option<Vec<String>>> {
        if self.spec.probe_served_models {
            super::opencode_models::probe_served_models()
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
#[path = "opencode_overlay_tests.rs"]
mod tests;

