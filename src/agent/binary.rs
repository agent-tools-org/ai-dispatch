// Host PATH / program resolution for agent dispatch preflight.
// Exports: ensure_*_available, built_in_binaries, builtin_binary_owner.
// Deps: AgentKind, env::which_exists.
use anyhow::Result;
use crate::types::AgentKind;
use super::env;

pub(crate) fn ensure_agent_binary_available(agent_kind: AgentKind, agent_name: &str) -> Result<()> {
    ensure_agent_binary_available_with(agent_kind, agent_name, env::which_exists)
}

pub(crate) fn ensure_agent_binary_available_with<F>(
    agent_kind: AgentKind,
    agent_name: &str,
    which: F,
) -> Result<()>
where
    F: Fn(&str) -> bool,
{
    if built_in_agent_binary_exists(agent_kind, which) {
        return Ok(());
    }
    let binary = built_in_binaries(agent_kind)
        .first()
        .copied()
        .unwrap_or(agent_name);
    anyhow::bail!(
        "Agent '{}' not found: binary '{}' missing from PATH",
        agent_name,
        binary
    );
}

/// Refuse dispatch when the resolved program from `build_command` is not runnable.
pub(crate) fn ensure_resolved_binary_available(agent_name: &str, program: &str) -> Result<()> {
    ensure_resolved_binary_available_with(agent_name, program, env::which_exists)
}

pub(crate) fn ensure_resolved_binary_available_with<F>(
    agent_name: &str,
    program: &str,
    which: F,
) -> Result<()>
where
    F: Fn(&str) -> bool,
{
    if resolved_binary_exists(program, &which) {
        return Ok(());
    }
    let binary = std::path::Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    anyhow::bail!(
        "Agent '{}' not found: binary '{}' missing from PATH",
        agent_name,
        binary
    );
}

fn resolved_binary_exists<F>(program: &str, which: &F) -> bool
where
    F: Fn(&str) -> bool,
{
    let path = std::path::Path::new(program);
    if path.is_absolute() || program.contains('/') || program.contains('\\') {
        return path.is_file();
    }
    which(program)
}

/// The binaries a built-in adapter may invoke. Single source of truth: the
/// PATH preflight and the custom-agent guard both read it, so a new agent
/// cannot be reachable by one and invisible to the other.
pub(crate) fn built_in_binaries(agent_kind: AgentKind) -> &'static [&'static str] {
    match agent_kind {
        AgentKind::Antigravity => &["agy"],
        AgentKind::Codex => &["codex"],
        AgentKind::CommandCode => &["commandcode"],
        AgentKind::Copilot => &["copilot"],
        AgentKind::Cursor => &["agent", "cursor-agent"],
        AgentKind::Gemini => &["gemini"],
        AgentKind::Qwen => &["qwen"],
        AgentKind::OpenCode => &["opencode"],
        AgentKind::Kilo => &["kilo"],
        AgentKind::MiMoCode => &["mimo"],
        AgentKind::Droid => &["droid"],
        AgentKind::Oz => &["oz"],
        AgentKind::Claude => &["claude"],
        AgentKind::Grok => &["grok"],
        AgentKind::Custom => &[],
    }
}

/// The built-in agent a bare command name belongs to, if any.
///
/// A custom agent naming one of these is a fork of that adapter, not a new
/// route: it re-declares the invocation by hand and therefore inherits none of
/// the adapter's flags, event parsing, quota accounting, model attribution or
/// session resume, and it reports `provider = unknown` while spending the
/// built-in's quota.
pub(crate) fn builtin_binary_owner(command: &str) -> Option<AgentKind> {
    let name = std::path::Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(command);
    AgentKind::ALL_BUILTIN
        .iter()
        .copied()
        .find(|kind| built_in_binaries(*kind).contains(&name))
}

pub(crate) fn built_in_agent_binary_exists<F>(agent_kind: AgentKind, which: F) -> bool
where
    F: Fn(&str) -> bool,
{
    if matches!(agent_kind, AgentKind::Custom) {
        return true;
    }
    built_in_binaries(agent_kind).iter().any(|name| which(name))
}
