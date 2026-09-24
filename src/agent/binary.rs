// Host PATH / program resolution: the one "can this route run here" predicate.
// Exports: route_blocker*, route_inventory, detect_agents, ensure_*_available, built_in_binaries.
// Deps: AgentKind, env::which_exists.
use anyhow::Result;
use crate::types::AgentKind;
use super::env;

#[path = "binary_route.rs"]
mod route;
pub(crate) use route::{RouteBlocker, custom_route_blocker, route_inventory, routable_builtins};
use route::route_blocker_with;
pub use route::detect_agents;
#[cfg(test)]
pub(crate) use route::DetectAgentsGuard;

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
    match route_blocker_with(agent_kind, which) {
        None => Ok(()),
        Some(blocker) => anyhow::bail!("Agent '{}' not found: {}", agent_name, blocker.detail()),
    }
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
    let blocker = route::RouteBlocker::NotInstalled {
        binary: std::path::Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(program)
            .to_string(),
    };
    anyhow::bail!("Agent '{}' not found: {}", agent_name, blocker.detail());
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
