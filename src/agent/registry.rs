// Custom agent registry: loads agent definitions from ~/.aid/agents/*.toml.
// Exports: load_custom_agents, resolve_custom_agent, list_custom_agents.
// Deps: crate::support::paths, super::custom.
#![allow(dead_code)]

use super::custom::{parse_config, CustomAgent, CustomAgentConfig};
use super::opencode_overlay::{OpenCodeOverlayAgent, OpenCodeOverlaySpec};
use crate::support::paths;
use crate::types::AgentKind;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

fn agents_dir() -> PathBuf {
    paths::aid_dir().join("agents")
}

fn load_from_dir(dir: &Path) -> HashMap<String, CustomAgentConfig> {
    let mut agents = HashMap::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }
            match fs::read_to_string(&path) {
                Ok(contents) => match parse_config(&contents) {
                    Ok(config) => {
                        if let Some(reason) = fork_of_builtin_reason(&config) {
                            aid_warn!("Ignoring {}: {}", path.display(), reason);
                            continue;
                        }
                        let id = config.id.clone();
                        agents.insert(id, config);
                    }
                    Err(err) => {
                        aid_warn!("Failed to parse {}: {}", path.display(), err);
                    }
                },
                Err(err) => {
                    aid_warn!("Failed to read {}: {}", path.display(), err);
                }
            }
        }
    }
    agents
}

/// A custom agent declares a route aid cannot otherwise reach: a different CLI,
/// or a different provider/model behind a wrapper. Naming a built-in's own
/// binary declares no new route — it re-implements that adapter by hand and
/// loses everything the adapter knows. Measured on droid: the built-in sends
/// `exec --output-format stream-json --skip-permissions-unsafe`, while four
/// forks sent the bare binary, which opens droid's interactive TUI and asks
/// "Trust this folder?" on a worktree path that is new every dispatch.
///
/// The two supported replacements are named in the message because they are
/// what the author actually wanted: `--skill` for a persona on a real route,
/// `delegate_to` + `forced_model` for a different model on a built-in CLI.
fn fork_of_builtin_reason(config: &CustomAgentConfig) -> Option<String> {
    if config.delegate_to.is_some() {
        return None;
    }
    let owner = super::builtin_binary_owner(&config.command)?;
    Some(format!(
        "custom agent '{}' runs the built-in '{}' binary ('{}'), so it is a fork of that adapter, not a new route — \
it inherits none of the adapter's flags, event parsing, quota accounting or session resume, and reports provider=unknown while spending {}'s quota. \
For a persona use `--skill {}` on a real route; for a different model on that CLI use `delegate_to` + `forced_model`.",
        config.id,
        owner.as_str(),
        config.command,
        owner.as_str(),
        config.id,
    ))
}

fn load_registry() -> HashMap<String, CustomAgentConfig> {
    load_custom_agents()
}

pub fn load_custom_agents() -> HashMap<String, CustomAgentConfig> {
    let mut agents = load_from_dir(&agents_dir());
    match crate::config::load_config() {
        Ok(config) => for route in config.api_route {
            agents.insert(route.id.clone(), super::api_route::registry_config(&route));
        },
        Err(error) => aid_warn!("Cannot load API routes: {error:#}"),
    }
    agents
}

fn resolve_from_registry(
    registry: &HashMap<String, CustomAgentConfig>,
    name: &str,
) -> Option<Box<dyn super::Agent>> {
    registry.get(name)
        .filter(|config| config.delegate_to.as_deref() != Some("claude"))
        .map(|config| build_agent(config))
}

fn build_agent(config: &CustomAgentConfig) -> Box<dyn super::Agent> {
    if let (Some(target), Some(model)) = (config.delegate_to.as_deref(), config.forced_model.as_deref())
        && target == "opencode"
    {
        return Box::new(OpenCodeOverlayAgent::from_spec(opencode_spec(config, model)))
            as Box<dyn super::Agent>;
    }
    if config.delegate_to.is_some() && config.forced_model.is_none() {
        aid_warn!(
            "[aid] Custom agent '{}' has delegate_to but no forced_model; falling back to bash wrapper.",
            config.id
        );
    }
    Box::new(CustomAgent {
        config: config.clone(),
    }) as Box<dyn super::Agent>
}

fn opencode_spec(config: &CustomAgentConfig, model: &str) -> OpenCodeOverlaySpec {
    OpenCodeOverlaySpec {
        id: config.id.clone(),
        display_name: config.display_name.clone(),
        reported_kind: AgentKind::Custom,
        binary: config.binary.clone().unwrap_or_else(|| "opencode".to_string()),
        extra_args: config.extra_args.clone(),
        default_model: Some(model.to_string()),
        interactive_input: config.interactive_input,
        rate_limit_kind: parse_rate_limit_kind(config),
        allow_external_directories: true,
        probe_served_models: false,
    }
}

fn parse_rate_limit_kind(config: &CustomAgentConfig) -> AgentKind {
    match config.rate_limit_kind.as_deref() {
        Some(kind) => AgentKind::parse_str(kind).unwrap_or_else(|| {
            aid_warn!(
                "[aid] Custom agent '{}' has unknown rate_limit_kind '{}'; using opencode.",
                config.id,
                kind
            );
            AgentKind::OpenCode
        }),
        None => AgentKind::OpenCode,
    }
}

pub fn resolve_custom_agent(name: &str) -> Option<Box<dyn super::Agent>> {
    let config = crate::config::load_config().ok()?;
    if let Some(route) = config.api_route.into_iter().find(|route| route.id == name) {
        return Some(Box::new(super::api_route::ApiRouteAgent(route)));
    }
    let registry = load_from_dir(&agents_dir());
    resolve_from_registry(&registry, name)
}

fn list_from_registry(registry: &HashMap<String, CustomAgentConfig>) -> Vec<CustomAgentConfig> {
    let mut agents: Vec<_> = registry.values().cloned().collect();
    agents.sort_by(|a, b| a.id.cmp(&b.id));
    agents
}

pub fn list_custom_agents() -> Vec<CustomAgentConfig> {
    let registry = load_registry();
    list_from_registry(&registry)
}

pub fn custom_agent_exists(name: &str) -> bool {
    let custom_file = agents_dir().join(format!("{name}.toml"));
    custom_file.is_file() || load_registry().contains_key(name)
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
