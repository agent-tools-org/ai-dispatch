// Custom agent registry: loads agent definitions from ~/.aid/agents/*.toml.
// Exports: load_custom_agents, resolve_custom_agent, list_custom_agents.
// Deps: crate::paths, super::custom.
#![allow(dead_code)]

use super::custom::{parse_config, CustomAgent, CustomAgentConfig};
use super::opencode_overlay::OpenCodeOverlayAgent;
use crate::paths;
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
        return Box::new(OpenCodeOverlayAgent::new(
            config.id.clone(),
            config.display_name.clone(),
            model.to_string(),
        )) as Box<dyn super::Agent>;
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
