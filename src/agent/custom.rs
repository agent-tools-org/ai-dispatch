//! CLI adapter for user-defined agents loaded from TOML.
//! Exports: CustomAgentConfig, CustomAgent, parse_config.
//! Deps: serde, toml, serde_json, chrono, std::process::Command, crate::types.
#![allow(dead_code)]

use anyhow::Result;
use chrono::Local;
use serde::Deserialize;
use serde_json::Value;
use std::process::Command;

use super::read_only::read_only_prompt;
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
    /// Whether aid may send responses, replies, and steering to the running CLI.
    /// Defaults to true to preserve the historical custom-agent behavior.
    #[serde(default = "default_interactive_input")]
    pub interactive_input: bool,
    #[serde(default = "default_output_format")]
    pub output_format: String,
    #[serde(default)]
    pub capabilities: CapabilityScores,
    /// Legacy hand-set label. Ignored for display and for `--egress` admission:
    /// egress is established from [`Self::base_url`] (or is Unknown). Kept so
    /// existing agent TOML still parses.
    #[serde(default = "default_trust_tier")]
    pub trust_tier: String,
    /// OpenAI-compatible endpoint for this agent, when known. A loopback host
    /// is the only evidence that admits `--egress local`.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Declared metering entity for this route. Never inferred from the host.
    #[serde(default)]
    pub provider: Option<String>,
    /// Declared metering shape (`none`, `account_pool`, ...). Stays unknown when unset.
    #[serde(default)]
    pub metering: Option<String>,
    #[serde(default)]
    pub strengths: Vec<String>,
    /// When set (currently only "opencode"), the registry returns a
    /// declarative overlay around the named engine instead of CustomAgent.
    #[serde(default)]
    pub delegate_to: Option<String>,
    #[serde(default)]
    pub forced_model: Option<String>,
    #[serde(default)]
    pub binary: Option<String>,
    #[serde(default)]
    pub extra_args: Vec<String>,
    #[serde(default)]
    pub rate_limit_kind: Option<String>,
}

fn default_trust_tier() -> String {
    "api".to_string()
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

fn default_interactive_input() -> bool {
    true
}

fn default_score() -> i32 {
    3
}

pub struct CustomAgent {
    pub config: CustomAgentConfig,
}

impl super::Agent for CustomAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Custom
    }

    fn rate_limit_name(&self) -> Option<&str> {
        Some(self.config.id.as_str())
    }

    fn streaming(&self) -> bool {
        self.config.streaming
    }

    fn accepts_interactive_input(&self) -> bool {
        self.config.interactive_input
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let effective_prompt = if opts.read_only {
            read_only_prompt(prompt, opts)
        } else {
            prompt.to_string()
        };
        if opts.read_only {
            aid_warn!("[aid] ⚠ Custom agent read-only does not sandbox the agent; aid fails the task if the Git run directory changes.");
        }
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

        if let Some(ref model) = opts.model
            && !self.config.model_flag.is_empty()
        {
            cmd.args([&self.config.model_flag, model]);
        }

        if let Some(ref output) = opts.output
            && !self.config.output_flag.is_empty()
        {
            cmd.args([&self.config.output_flag, output]);
        }

        match self.config.prompt_mode.as_str() {
            "flag" if !self.config.prompt_flag.is_empty() => {
                cmd.args([&self.config.prompt_flag, &effective_prompt]);
            }
            "flag" => {
                cmd.arg(&effective_prompt);
            }
            "stdin" => {}
            _ => {
                cmd.arg(&effective_prompt);
            }
        }

        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }

        if self.config.output_format == "jsonl"
            && let Ok(value) = serde_json::from_str::<Value>(trimmed)
        {
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

        Some(TaskEvent {
            task_id: task_id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Reasoning,
            detail: super::truncate::truncate_text(trimmed, 120),
            metadata: None,
        })
    }
}

pub fn parse_config(toml_content: &str) -> Result<CustomAgentConfig> {
    let file: CustomAgentFile = toml::from_str(toml_content)?;
    super::removed::reject(&file.agent.id)?;
    if let Some(target) = file.agent.delegate_to.as_deref() { super::removed::reject(target)?; }
    Ok(file.agent)
}

#[cfg(test)]
#[path = "custom_tests.rs"]
mod tests;
