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
    pub rate_limit_kind: AgentKind,
    pub allow_external_directories: bool,
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
            rate_limit_kind: AgentKind::OpenCode,
            allow_external_directories: true,
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

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        if opts.read_only && self.spec.reported_kind == AgentKind::Custom {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{paths, rate_limit};

    fn base_opts() -> RunOpts {
        RunOpts {
            dir: None,
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: false,
            sandbox: false,
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

    #[test]
    fn overlay_spec_uses_binary_args_kind_and_rate_limit_kind() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = paths::AidHomeGuard::set(temp.path());
        rate_limit::clear_rate_limit(&AgentKind::MiMoCode, None);
        rate_limit::clear_rate_limit(&AgentKind::OpenCode, None);
        let agent = OpenCodeOverlayAgent::from_spec(OpenCodeOverlaySpec {
            id: "mimocode".into(),
            display_name: "MiMo Code".into(),
            reported_kind: AgentKind::MiMoCode,
            binary: "mimo".into(),
            extra_args: vec!["--dangerously-skip-permissions".into()],
            default_model: Some("mimo/mimo-auto".into()),
            rate_limit_kind: AgentKind::MiMoCode,
            allow_external_directories: false,
        });
        let cmd = agent.build_command("hi", &base_opts()).unwrap();
        assert_eq!(agent.kind(), AgentKind::MiMoCode);
        assert_eq!(cmd.get_program().to_string_lossy(), "mimo");
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["-m", "mimo/mimo-auto"]));
        let event = agent
            .parse_event(
                &TaskId("t-mimo".into()),
                r#"{"type":"error","error":{"name":"APIError","data":{"message":"Insufficient balance. Manage your billing here"}}}"#,
            )
            .unwrap();
        assert_eq!(event.event_kind, EventKind::Error);
        assert!(rate_limit::is_rate_limited(&AgentKind::MiMoCode, None));
        assert!(!rate_limit::is_rate_limited(&AgentKind::OpenCode, None));
        rate_limit::clear_rate_limit(&AgentKind::MiMoCode, None);
        rate_limit::clear_rate_limit(&AgentKind::OpenCode, None);
    }

    #[test]
    fn custom_overlay_marks_its_own_id_not_opencode() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = paths::AidHomeGuard::set(temp.path());
        let agent = OpenCodeOverlayAgent::from_spec(OpenCodeOverlaySpec {
            id: "auditor".into(),
            display_name: "Auditor".into(),
            reported_kind: AgentKind::Custom,
            binary: "opencode".into(),
            extra_args: Vec::new(),
            default_model: Some("x".into()),
            rate_limit_kind: AgentKind::OpenCode,
            allow_external_directories: true,
        });
        let _ = agent.parse_event(
            &TaskId("t-aud".into()),
            r#"{"type":"error","error":{"name":"APIError","data":{"message":"Insufficient balance. Manage your billing here"}}}"#,
        );
        assert!(rate_limit::is_rate_limited(&AgentKind::Custom, Some("auditor")));
        assert!(!rate_limit::is_rate_limited(&AgentKind::OpenCode, None));
        assert!(!rate_limit::is_rate_limited(&AgentKind::Custom, Some("other")));
    }
}
