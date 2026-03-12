// Agent trait and registry for AI CLI adapters.
// Each agent knows how to build its CLI command and parse its output.

pub mod codex;
pub mod gemini;

use anyhow::Result;
use std::process::Command;

use crate::types::*;

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
}

/// Options passed to agent for command construction
#[derive(Debug, Clone)]
pub struct RunOpts {
    pub dir: Option<String>,
    pub output: Option<String>,
    pub model: Option<String>,
}

/// Detect which agents are installed on the system
pub fn detect_agents() -> Vec<AgentKind> {
    let mut found = Vec::new();
    for (name, kind) in [
        ("gemini", AgentKind::Gemini),
        ("codex", AgentKind::Codex),
        ("opencode", AgentKind::OpenCode),
    ] {
        if which_exists(name) {
            found.push(kind);
        }
    }
    found
}

/// Get an agent adapter by kind
pub fn get_agent(kind: AgentKind) -> Box<dyn Agent> {
    match kind {
        AgentKind::Codex => Box::new(codex::CodexAgent),
        AgentKind::Gemini => Box::new(gemini::GeminiAgent),
        AgentKind::OpenCode => todo!("OpenCode adapter not yet implemented"),
    }
}

fn which_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
