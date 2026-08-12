// Agent trait and registry for AI CLI adapters.
// Each agent knows how to build its CLI command and parse its output.

pub mod antigravity;
pub mod claude;
pub(crate) mod claude_events;
pub mod commandcode;
pub mod codex;
pub mod copilot;
pub mod cursor;
pub mod droid;
pub mod gemini;
pub(crate) mod gemini_support;
pub mod grok;
pub mod kilo;
pub(crate) mod model_group;
pub(crate) mod model_validation;
pub mod mimocode;
pub mod opencode;
pub(crate) mod opencode_overlay;
pub mod oz;
pub mod qwen;
pub(crate) mod read_only;
pub(crate) mod custom;
pub(crate) mod cargo_target;
pub(crate) mod egress;
pub(crate) mod registry;
pub mod classifier;
pub(crate) mod selection;
pub(crate) mod stream_completion;
pub(crate) mod truncate;
pub(crate) mod response;
use anyhow::Result;
use std::collections::HashMap;
use std::process::Command;

use crate::prompt_scan::scan_for_injection;
use crate::store;
use crate::types::*;
pub mod home_isolation;

pub(crate) mod env;
#[path = "binary.rs"]
mod binary;
pub(crate) use binary::{
    builtin_binary_owner, ensure_agent_binary_available, ensure_agent_binary_available_with,
    ensure_resolved_binary_available, ensure_resolved_binary_available_with,
};
pub(crate) use response::extract_response;
#[cfg(test)]
pub(crate) use binary::built_in_agent_binary_exists;
#[allow(unused_imports)]
pub use env::{
    agent_has_fs_access, apply_cargo_target_env, apply_codex_home_env, apply_run_env, apply_rust_build_cache_env,
    cargo_target_env_arg, is_rust_project, rust_build_cache_target_dir, set_git_ceiling,
    shared_target_dir, target_dir_for_worktree,
};
pub(crate) use env::{should_use_durable_codex_home, CliCommandOutput, CliCommandRunner};
/// Adapter trait for AI CLI tools
pub trait Agent: Send + Sync {
    fn kind(&self) -> AgentKind;
    /// Custom-agent id used as the rate-limit marker slug. Built-ins return
    /// `None` so markers stay `rate-limit-{as_str()}`.
    fn rate_limit_name(&self) -> Option<&str> {
        None
    }

    /// Whether this agent streams JSONL (true) or outputs a single JSON blob (false)
    fn streaming(&self) -> bool;
    /// Whether the running CLI consumes interactive input from its PTY.
    fn accepts_interactive_input(&self) -> bool;

    /// Interactive agents that read stdin mid-run can be nudged to unstick.
    /// An adapter may narrow this for agents that must not receive automatic nudges,
    /// but it can never widen the interactive-input capability.
    fn accepts_idle_nudge(&self) -> bool {
        self.accepts_interactive_input()
    }

    /// Build the OS command to execute this agent
    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command>;

    /// Validate the installed host CLI contract before a task row is claimed; use `validate_cli_with` for injected runners.
    fn validate_cli(&self) -> Result<()> {
        Ok(())
    }
    fn validate_cli_with(&self, _run: &CliCommandRunner<'_>) -> Result<()> { self.validate_cli() }
    /// Build a command with dispatch facts that affect agent-specific behavior.
    fn build_command_with_context(
        &self,
        prompt: &str,
        opts: &RunOpts,
        _context: CommandContext,
    ) -> Result<Command> {
        self.build_command(prompt, opts)
    }

    /// Parse a single line of output into an event (streaming agents only)
    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent>;

    /// Parse full agent output into completion info.
    /// Called by buffered finalize always, and by streaming finalize when exit code is 0.
    fn parse_completion(&self, _output: &str) -> CompletionInfo {
        CompletionInfo {
            tokens: None,
            status: TaskStatus::Done,
            model: None,
            cost_usd: None,
            exit_code: None,
        }
    }

    /// Whether this agent requires a PTY even for foreground execution.
    fn needs_pty(&self) -> bool {
        false
    }

    /// Query served models from CLI or local config.
    /// Returns Ok(Some(list)) if positively known, or Ok(None) if unqueryable.
    fn served_models(&self) -> Result<Option<Vec<String>>> {
        Ok(None)
    }
}

#[derive(Debug, Clone, Default)]
pub struct CommandContext {
    pub durable_codex_home: bool,
    pub cargo_target_dir: Option<String>,
}

/// Options passed to agent for command construction
/// Env key naming the log aid will watch for proof the agent is alive.
pub const AGENT_LOG_ENV: &str = "AID_AGENT_LOG";

/// Hand an agent the log path aid will watch — but only when aid can read it back.
///
/// Sandbox remaps `AID_HOME`; containers do not mount `~/.aid`. A host path would be
/// unwritable inside and empty outside, so aid skips seeding. Buffered liveness does
/// not cover sandbox and container runs. Decision stays here so adapters stay dumb.
pub fn env_with_agent_log(
    env: Option<HashMap<String, String>>,
    task_id: &str,
    watchable: bool,
) -> Option<HashMap<String, String>> {
    if !watchable {
        return env;
    }
    let mut env = env.unwrap_or_default();
    env.insert(
        AGENT_LOG_ENV.to_string(),
        crate::paths::agent_log_path(task_id).to_string_lossy().into_owned(),
    );
    Some(env)
}

/// The log path aid promised to watch, if it gave one.
pub fn agent_log_from_opts(opts: &RunOpts) -> Option<&str> {
    opts.env.as_ref()?.get(AGENT_LOG_ENV).map(String::as_str).filter(|p| !p.is_empty())
}

#[derive(Debug, Clone)]
pub struct RunOpts {
    pub dir: Option<String>,
    pub output: Option<String>,
    pub result_file: Option<String>,
    pub model: Option<String>,
    pub budget: bool,
    pub read_only: bool,
    pub sandbox: bool,
    pub context_files: Vec<String>,
    pub session_id: Option<String>,
    pub env: Option<HashMap<String, String>>,
    pub env_forward: Option<Vec<String>>,
}

/// Detect which agents are installed on the system
pub fn detect_agents() -> Vec<AgentKind> {
    #[cfg(test)]
    {
        let maybe = DETECT_AGENTS_OVERRIDE.with(|cell| cell.borrow().clone());
        if let Some(list) = maybe {
            return list;
        }
    }
    let mut found = Vec::new();
    for (name, kind) in [
        ("gemini", AgentKind::Gemini),
        ("agy", AgentKind::Antigravity),
        ("qwen", AgentKind::Qwen),
        ("codex", AgentKind::Codex),
        ("commandcode", AgentKind::CommandCode),
        ("opencode", AgentKind::OpenCode),
        ("copilot", AgentKind::Copilot),
        ("agent", AgentKind::Cursor),
        ("cursor-agent", AgentKind::Cursor),
        ("droid", AgentKind::Droid),
        ("kilo", AgentKind::Kilo),
        ("mimo", AgentKind::MiMoCode),
        ("oz", AgentKind::Oz),
        ("claude", AgentKind::Claude),
        ("grok", AgentKind::Grok),
    ] {
        if env::which_exists(name) && !found.contains(&kind) {
            found.push(kind);
        }
    }
    found
}

#[cfg(test)]
std::thread_local! {
    static DETECT_AGENTS_OVERRIDE: std::cell::RefCell<Option<Vec<AgentKind>>> =
        const { std::cell::RefCell::new(None) };
}

/// RAII guard that pins `detect_agents()` to a test-supplied list on the
/// current thread. Restores the previous value on drop so nested scopes
/// compose correctly.
#[cfg(test)]
pub(crate) struct DetectAgentsGuard {
    previous: Option<Vec<AgentKind>>,
}

#[cfg(test)]
impl DetectAgentsGuard {
    pub fn set(agents: Vec<AgentKind>) -> Self {
        let previous = DETECT_AGENTS_OVERRIDE.with(|cell| cell.borrow().clone());
        DETECT_AGENTS_OVERRIDE.with(|cell| *cell.borrow_mut() = Some(agents));
        Self { previous }
    }
}

#[cfg(test)]
impl Drop for DetectAgentsGuard {
    fn drop(&mut self) {
        DETECT_AGENTS_OVERRIDE.with(|cell| *cell.borrow_mut() = self.previous.take());
    }
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
        AgentKind::Antigravity => Box::new(antigravity::AntigravityAgent),
        AgentKind::Codex => Box::new(codex::CodexAgent),
        AgentKind::CommandCode => Box::new(commandcode::CommandCodeAgent),
        AgentKind::Copilot => Box::new(copilot::CopilotAgent),
        AgentKind::Cursor => Box::new(cursor::CursorAgent),
        AgentKind::Gemini => Box::new(gemini::GeminiAgent),
        AgentKind::Qwen => Box::new(qwen::QwenAgent),
        AgentKind::OpenCode => Box::new(opencode::OpenCodeAgent),
        AgentKind::Kilo => Box::new(kilo::agent()),
        AgentKind::MiMoCode => Box::new(mimocode::agent()),
        AgentKind::Droid => Box::new(droid::DroidAgent),
        AgentKind::Oz => Box::new(oz::OzAgent),
        AgentKind::Claude => Box::new(claude::ClaudeAgent),
        AgentKind::Grok => Box::new(grok::GrokAgent),
        AgentKind::Custom => panic!("Custom agents must be resolved via resolve_agent()"),
    }
}

/// Embed context file contents into the prompt text for agents without native context file flags.
pub fn embed_context_in_prompt(prompt: &str, context_files: &[String]) -> anyhow::Result<String> {
    if context_files.is_empty() {
        return Ok(prompt.to_string());
    }
    let mut combined = prompt.to_string();
    for file in context_files {
        let contents = std::fs::read_to_string(file)?;
        let scan = scan_for_injection(&contents);
        for warning in &scan.warnings {
            aid_warn!(
                "[aid] ⚠ Context file {file}: {} (line {})",
                warning.pattern,
                warning.line_num
            );
        }
        if scan.has_critical {
            aid_warn!("[aid] ⚠ Critical injection pattern detected in {file} — content may be adversarial");
        }
        combined.push_str("\n\n[Context File: ");
        combined.push_str(file);
        combined.push_str("]\n");
        combined.push_str(&contents);
    }
    Ok(combined)
}

#[cfg(test)]
mod cursor_binary_tests;
#[cfg(test)]
mod binary_preflight_tests;
#[cfg(test)]
mod tests;
