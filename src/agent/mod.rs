// Agent trait and registry for AI CLI adapters.
// Each agent knows how to build its CLI command and parse its output.

pub mod claude;
pub(crate) mod claude_events;
pub mod codebuff;
pub mod codex;
pub mod cursor;
pub mod droid;
pub mod gemini;
pub mod kilo;
pub mod opencode;
pub mod oz;
pub(crate) mod custom;
pub(crate) mod registry;
pub mod classifier;
pub(crate) mod selection;
pub(crate) mod truncate;

use anyhow::Result;
use std::collections::HashMap;
use std::process::Command;

use crate::store;
use crate::types::*;

pub(crate) mod env;
#[allow(unused_imports)]
pub use env::{
    agent_has_fs_access, apply_run_env, is_rust_project, set_git_ceiling, shared_target_dir,
    target_dir_for_worktree,
};

/// Adapter trait for AI CLI tools
pub trait Agent: Send + Sync {
    fn kind(&self) -> AgentKind;

    /// Whether this agent streams JSONL (true) or outputs a single JSON blob (false)
    fn streaming(&self) -> bool;

    /// Build the OS command to execute this agent
    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command>;

    /// Parse a single line of output into an event (streaming agents only)
    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent>;

    /// Parse buffered output into completion info (non-streaming agents)
    fn parse_completion(&self, output: &str) -> CompletionInfo;

    /// Whether this agent requires a PTY even for foreground execution.
    /// Agents that don't produce stdout when piped (e.g. opencode) should return true.
    fn needs_pty(&self) -> bool {
        false
    }
}

/// Options passed to agent for command construction
#[derive(Debug, Clone)]
pub struct RunOpts {
    pub dir: Option<String>,
    pub output: Option<String>,
    pub result_file: Option<String>,
    pub model: Option<String>,
    pub budget: bool,
    pub read_only: bool,
    pub context_files: Vec<String>,
    pub session_id: Option<String>,
    pub env: Option<HashMap<String, String>>,
    pub env_forward: Option<Vec<String>>,
}

/// Detect which agents are installed on the system
pub fn detect_agents() -> Vec<AgentKind> {
    let mut found = Vec::new();
    for (name, kind) in [
        ("gemini", AgentKind::Gemini),
        ("codex", AgentKind::Codex),
        ("opencode", AgentKind::OpenCode),
        ("agent", AgentKind::Cursor),
        ("cursor-agent", AgentKind::Cursor),
        ("droid", AgentKind::Droid),
        ("kilo", AgentKind::Kilo),
        ("aid-codebuff", AgentKind::Codebuff),
        ("oz", AgentKind::Oz),
        ("claude", AgentKind::Claude),
    ] {
        if env::which_exists(name) && !found.contains(&kind) {
            found.push(kind);
        }
    }
    found
}

pub(crate) fn select_agent_with_reason(
    prompt: &str, opts: &RunOpts, store: &store::Store,
    team: Option<&crate::team::TeamConfig>,
) -> (String, String) {
    selection::select_agent_with_reason(prompt, opts, store, team)
}

/// Get an agent adapter by kind
pub fn get_agent(kind: AgentKind) -> Box<dyn Agent> {
    match kind {
        AgentKind::Codex => Box::new(codex::CodexAgent),
        AgentKind::Cursor => Box::new(cursor::CursorAgent),
        AgentKind::Gemini => Box::new(gemini::GeminiAgent),
        AgentKind::OpenCode => Box::new(opencode::OpenCodeAgent),
        AgentKind::Kilo => Box::new(kilo::KiloAgent),
        AgentKind::Codebuff => Box::new(codebuff::CodebuffAgent),
        AgentKind::Droid => Box::new(droid::DroidAgent),
        AgentKind::Oz => Box::new(oz::OzAgent),
        AgentKind::Claude => Box::new(claude::ClaudeAgent),
        AgentKind::Custom => panic!("Custom agents must be resolved via resolve_agent()"),
    }
}

#[cfg(test)]
mod cursor_binary_tests;
#[cfg(test)]
mod tests;
