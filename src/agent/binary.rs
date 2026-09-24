// Host PATH / program resolution: the one "can this route run here" predicate.
// Exports: route_blocker*, route_inventory, detect_agents, ensure_*_available, built_in_program,
// built_in_binaries.
// Deps: AgentKind, env::which_exists, cursor::cursor_binary.
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
    match program_blocker(program, &which) {
        None => Ok(()),
        Some(blocker) => anyhow::bail!("Agent '{}' not found: {}", agent_name, blocker.detail()),
    }
}

/// The one "can this program run here" predicate: route inventory, advise, the
/// dispatch guards and the command preflight all end here.
fn program_blocker<F>(program: &str, which: &F) -> Option<route::RouteBlocker>
where
    F: Fn(&str) -> bool,
{
    if !program.is_empty() && resolved_binary_exists(program, which) {
        return None;
    }
    let binary = std::path::Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program)
        .to_string();
    Some(route::RouteBlocker::NotInstalled { binary })
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

/// The program a built-in adapter spawns on this host: the same resolution its
/// `build_command` uses, including Cursor's identity check on `agent`.
pub(crate) fn built_in_program(agent_kind: AgentKind) -> Option<&'static str> {
    match agent_kind {
        AgentKind::Custom => None,
        AgentKind::Cursor => Some(super::cursor::cursor_binary()),
        _ => built_in_binaries(agent_kind).first().copied(),
    }
}

/// Every binary name a built-in adapter may invoke. The custom-agent guard reads
/// it; eligibility resolves the one program the adapter spawns via `built_in_program`.
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
