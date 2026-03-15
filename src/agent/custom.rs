//! CLI adapter for user-defined agents loaded from TOML.
//! Exports: CustomAgentConfig, CustomAgent, parse_config.
//! Deps: serde, toml, serde_json, chrono, std::process::Command, crate::types.
#![allow(dead_code)]

use anyhow::Result;
use chrono::Local;
use serde::Deserialize;
use serde_json::Value;
use std::process::Command;

use super::RunOpts;
use crate::types::*;

#[derive(Debug, Clone, Deserialize)]
pub struct CustomAgentFile {
    pub agent: CustomAgentConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CustomAgentConfig {
    pub id: String,
    pub display_name: String,
    pub command: String,
    #[serde(default = "default_prompt_mode")]
    pub prompt_mode: String,
    #[serde(default)]
    pub prompt_flag: String,
    #[serde(default)]
    pub dir_flag: String,
    #[serde(default)]
    pub model_flag: String,
    #[serde(default)]
    pub output_flag: String,
    #[serde(default)]
    pub fixed_args: Vec<String>,
    #[serde(default)]
    pub streaming: bool,
    #[serde(default = "default_output_format")]
    pub output_format: String,
    #[serde(default)]
    pub capabilities: CapabilityScores,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CapabilityScores {
    #[serde(default)]
    pub research: i32,
    #[serde(default = "default_score")]
    pub simple_edit: i32,
    #[serde(default = "default_score")]
    pub complex_impl: i32,
    #[serde(default)]
    pub frontend: i32,
    #[serde(default = "default_score")]
    pub debugging: i32,
    #[serde(default = "default_score")]
    pub testing: i32,
    #[serde(default = "default_score")]
    pub refactoring: i32,
    #[serde(default)]
    pub documentation: i32,
}

fn default_prompt_mode() -> String {
    "arg".to_string()
}

fn default_output_format() -> String {
    "text".to_string()
}

fn default_score() -> i32 {
    3
}

pub struct CustomAgent {
    pub config: CustomAgentConfig,
}

impl super::Agent for CustomAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Codex
    }

    fn streaming(&self) -> bool {
        self.config.streaming
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let mut cmd = Command::new(&self.config.command);

        for arg in &self.config.fixed_args {
            cmd.arg(arg);
        }

        if let Some(ref dir) = opts.dir {
            if !self.config.dir_flag.is_empty() {
                cmd.args([&self.config.dir_flag, dir]);
            }
            cmd.current_dir(dir);
        }

        if let Some(ref model) = opts.model {
            if !self.config.model_flag.is_empty() {
                cmd.args([&self.config.model_flag, model]);
            }
        }

        if let Some(ref output) = opts.output {
            if !self.config.output_flag.is_empty() {
                cmd.args([&self.config.output_flag, output]);
            }
        }

        match self.config.prompt_mode.as_str() {
            "flag" => {
                if !self.config.prompt_flag.is_empty() {
                    cmd.args([&self.config.prompt_flag, prompt]);
                } else {
                    cmd.arg(prompt);
                }
            }
            "stdin" => {}
            _ => {
                cmd.arg(prompt);
            }
        }

        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }

        if self.config.output_format == "jsonl" {
            if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
                let event_type = value
                    .get("type")
                    .or_else(|| value.get("event"))
                    .or_else(|| value.get("kind"))
                    .and_then(|v| v.as_str());
                if let Some(et) = event_type {
                    let detail = value
                        .get("message")
                        .or_else(|| value.get("text"))
                        .or_else(|| value.get("detail"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(et);
                    let kind = match et {
                        t if t.contains("error") => EventKind::Error,
                        t if t.contains("tool") => EventKind::ToolCall,
                        t if t.contains("complet") => EventKind::Completion,
                        _ => EventKind::Reasoning,
                    };
                    return Some(TaskEvent {
                        task_id: task_id.clone(),
                        timestamp: Local::now(),
                        event_kind: kind,
                        detail: super::truncate::truncate_text(detail, 120),
                        metadata: None,
                    });
                }
            }
        }

        Some(TaskEvent {
            task_id: task_id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Reasoning,
            detail: super::truncate::truncate_text(trimmed, 120),
            metadata: None,
        })
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

pub fn parse_config(toml_content: &str) -> Result<CustomAgentConfig> {
    let file: CustomAgentFile = toml::from_str(toml_content)?;
    Ok(file.agent)
}

#[cfg(test)]
mod tests {
    use super::super::Agent;
    use super::*;
    use crate::types::{EventKind, TaskId};

    fn base_opts() -> RunOpts {
        RunOpts {
            dir: None,
            output: None,
            model: None,
            budget: false,
            read_only: false,
            context_files: Vec::new(),
            session_id: None,
        }
    }

    fn base_config(command: &str) -> CustomAgentConfig {
        CustomAgentConfig {
            id: "custom".into(),
            display_name: "Custom Agent".into(),
            command: command.into(),
            prompt_mode: default_prompt_mode(),
            prompt_flag: String::new(),
            dir_flag: String::new(),
            model_flag: String::new(),
            output_flag: String::new(),
            fixed_args: Vec::new(),
            streaming: false,
            output_format: default_output_format(),
            capabilities: CapabilityScores::default(),
        }
    }

    #[test]
    fn parse_minimal_config() {
        let toml_data = r#"
            [agent]
            id = "aider"
            display_name = "Aider"
            command = "aider"
        "#;
        let config = parse_config(toml_data).unwrap();
        assert_eq!(config.id, "aider");
        assert_eq!(config.prompt_mode, "arg");
        assert_eq!(config.output_format, "text");
        assert_eq!(config.capabilities.simple_edit, 0);
    }

    #[test]
    fn parse_full_config() {
        let toml_data = r#"
            [agent]
            id = "walker"
            display_name = "Walker"
            command = "walker-cli"
            prompt_mode = "flag"
            prompt_flag = "--input"
            dir_flag = "--dir"
            model_flag = "--model"
            output_flag = "--out"
            fixed_args = ["--yes", "--batch"]
            streaming = true
            output_format = "jsonl"

            [agent.capabilities]
            research = 2
            simple_edit = 4
            complex_impl = 8
            frontend = 1
            debugging = 5
            testing = 6
            refactoring = 7
            documentation = 2
        "#;
        let config = parse_config(toml_data).unwrap();
        assert!(config.streaming);
        assert_eq!(config.prompt_mode, "flag");
        assert_eq!(config.capabilities.complex_impl, 8);
        assert_eq!(config.fixed_args.len(), 2);
    }

    #[test]
    fn build_command_with_arg_mode() {
        let mut config = base_config("agent-cli");
        config.fixed_args.push("--yes".into());
        let agent = CustomAgent { config };
        let cmd = agent.build_command("ask", &base_opts()).unwrap();
        let args: Vec<_> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, vec!["--yes", "ask"]);
    }

    #[test]
    fn build_command_with_flag_mode() {
        let mut config = base_config("agent-cli");
        config.prompt_mode = "flag".into();
        config.prompt_flag = "--message".into();
        config.fixed_args.push("--ready".into());
        let agent = CustomAgent { config };
        let args: Vec<_> = agent
            .build_command("prompt", &base_opts())
            .unwrap()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, vec!["--ready", "--message", "prompt"]);
    }

    #[test]
    fn build_command_with_dir() {
        let mut config = base_config("agent-cli");
        config.dir_flag = "--dir".into();
        let mut opts = base_opts();
        opts.dir = Some("/tmp/work".into());
        let cmd = CustomAgent { config }
            .build_command("prompt", &opts)
            .unwrap();
        let args: Vec<_> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(&args[..2], ["--dir".to_string(), "/tmp/work".to_string()]);
        let current_dir = cmd
            .get_current_dir()
            .map(|p| p.to_string_lossy().into_owned());
        assert_eq!(current_dir, Some("/tmp/work".to_string()));
    }

    #[test]
    fn parse_event_jsonl() {
        let mut config = base_config("agent-cli");
        config.output_format = "jsonl".into();
        let agent = CustomAgent { config };
        let task_id = TaskId("t-0001".into());
        let line = r#"{"type":"completion","message":"done"}"#;
        let event = agent.parse_event(&task_id, line).unwrap();
        assert_eq!(event.event_kind, EventKind::Completion);
        assert_eq!(event.detail, "done");
    }

    #[test]
    fn parse_event_text() {
        let config = base_config("agent-cli");
        let agent = CustomAgent { config };
        let task_id = TaskId("t-0002".into());
        let event = agent.parse_event(&task_id, " step ").unwrap();
        assert_eq!(event.event_kind, EventKind::Reasoning);
        assert_eq!(event.detail, "step");
    }
}
