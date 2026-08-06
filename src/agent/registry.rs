// Custom agent registry: loads agent definitions from ~/.aid/agents/*.toml.
// Exports: load_custom_agents, resolve_custom_agent, list_custom_agents.
// Deps: crate::paths, super::custom.
#![allow(dead_code)]

use super::custom::{parse_config, CustomAgent, CustomAgentConfig};
use super::opencode_overlay::{OpenCodeOverlayAgent, OpenCodeOverlaySpec};
use crate::paths;
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
    load_from_dir(&agents_dir())
}

pub fn load_custom_agents() -> HashMap<String, CustomAgentConfig> {
    load_from_dir(&agents_dir())
}

fn resolve_from_registry(
    registry: &HashMap<String, CustomAgentConfig>,
    name: &str,
) -> Option<Box<dyn super::Agent>> {
    registry.get(name).map(|config| build_agent(config))
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
        rate_limit_kind: parse_rate_limit_kind(config),
        allow_external_directories: true,
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
    let registry = load_registry();
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
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    fn write_agent(dir: &Path, file: &str, contents: &str) {
        fs::write(dir.join(file), contents).unwrap();
    }

    /// A fork of a built-in adapter is not a route and must not load. The
    /// negative half matters as much: a wrapper around an unrelated binary is
    /// exactly what custom agents are for and must still load.
    #[test]
    fn a_custom_agent_naming_a_builtin_binary_is_refused() {
        let dir = TempDir::new().unwrap();
        write_agent(
            dir.path(),
            "fork.toml",
            r#"[agent]
id = "l2-researcher"
display_name = "L2 Researcher"
command = "droid"
prompt_mode = "arg"
"#,
        );
        write_agent(
            dir.path(),
            "wrapper.toml",
            r#"[agent]
id = "goose"
display_name = "Goose"
command = "goose"
prompt_mode = "arg"
"#,
        );
        let agents = load_from_dir(dir.path());
        assert!(!agents.contains_key("l2-researcher"), "a droid fork must not load");
        assert!(agents.contains_key("goose"), "a genuinely different CLI must still load");
    }

    #[test]
    fn a_path_qualified_builtin_binary_is_refused_too() {
        let config = parse_config(
            r#"[agent]
id = "sneaky"
display_name = "Sneaky"
command = "/usr/local/bin/codex"
prompt_mode = "arg"
"#,
        )
        .unwrap();
        assert!(fork_of_builtin_reason(&config).is_some());
    }

    /// `delegate_to` is the supported way to reach a different model through a
    /// built-in CLI, so it is a route and stays allowed.
    #[test]
    fn a_delegating_agent_is_still_allowed() {
        let config = parse_config(
            r#"[agent]
id = "mimo"
display_name = "MiMo"
command = "opencode"
prompt_mode = "arg"
delegate_to = "opencode"
forced_model = "mimo/mimo-v2.5-pro"
"#,
        )
        .unwrap();
        assert!(fork_of_builtin_reason(&config).is_none());
    }

    fn sample_agent_toml(id: &str) -> String {
        format!(
            r#"[agent]
id = "{id}"
display_name = "{id} agent"
command = "{id}"
"#,
            id = id
        )
    }

    #[test]
    fn empty_dir_returns_empty_registry() {
        let dir = TempDir::new().unwrap();
        assert!(load_from_dir(dir.path()).is_empty());
    }

    #[test]
    fn loads_valid_toml() {
        let dir = TempDir::new().unwrap();
        write_agent(dir.path(), "foo.toml", &sample_agent_toml("foo"));
        let map = load_from_dir(dir.path());
        assert!(map.contains_key("foo"));
    }

    #[test]
    fn skips_invalid_toml() {
        let dir = TempDir::new().unwrap();
        write_agent(dir.path(), "bad.toml", "not = valid = toml");
        assert!(load_from_dir(dir.path()).is_empty());
    }

    #[test]
    fn resolve_returns_none_for_unknown() {
        let map = HashMap::new();
        assert!(resolve_from_registry(&map, "missing").is_none());
    }

    #[test]
    fn list_returns_sorted() {
        let dir = TempDir::new().unwrap();
        write_agent(dir.path(), "b.toml", &sample_agent_toml("b"));
        write_agent(dir.path(), "a.toml", &sample_agent_toml("a"));
        let map = load_from_dir(dir.path());
        let list = list_from_registry(&map);
        let ids: Vec<_> = list.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn delegate_to_opencode_returns_overlay_agent() {
        let toml_data = r#"[agent]
id = "mimo"
display_name = "MiMo"
command = "bash"
delegate_to = "opencode"
forced_model = "mimo/mimo-v2.5-pro"
"#;
        let config = parse_config(toml_data).unwrap();
        let agent = build_agent(&config);
        // Overlay reports Custom kind; bash CustomAgent does too now, so we
        // additionally verify that build_command produces an `opencode` invocation
        // with -m flag rather than a `bash -lc ...` wrapper.
        let opts = super::super::RunOpts {
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
        };
        let cmd = agent.build_command("hello", &opts).unwrap();
        let program = cmd.get_program().to_string_lossy().into_owned();
        assert_eq!(program, "opencode");
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.iter().any(|a| a == "-m"));
        assert!(args.iter().any(|a| a == "mimo/mimo-v2.5-pro"));
    }

    #[test]
    fn delegate_to_opencode_uses_binary_extra_args_and_rate_limit_kind() {
        let toml_data = r#"[agent]
id = "mimocode"
display_name = "MiMo Code"
command = "bash"
delegate_to = "opencode"
forced_model = "mimo/mimo-auto"
binary = "mimo"
extra_args = ["--dangerously-skip-permissions"]
rate_limit_kind = "mimocode"
"#;
        let config = parse_config(toml_data).unwrap();
        let agent = build_agent(&config);
        let opts = super::super::RunOpts {
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
        };
        let cmd = agent.build_command("hello", &opts).unwrap();
        assert_eq!(cmd.get_program().to_string_lossy(), "mimo");
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["-m", "mimo/mimo-auto"]));
    }

    #[test]
    fn missing_forced_model_falls_back_to_bash_wrapper() {
        let toml_data = r#"[agent]
id = "broken"
display_name = "Broken"
command = "bash"
delegate_to = "opencode"
"#;
        let config = parse_config(toml_data).unwrap();
        let agent = build_agent(&config);
        let opts = super::super::RunOpts {
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
        };
        let cmd = agent.build_command("hi", &opts).unwrap();
        // Falls back to plain CustomAgent → command is "bash"
        assert_eq!(cmd.get_program().to_string_lossy(), "bash");
    }
}
