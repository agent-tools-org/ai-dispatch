// Custom agent registry: loads agent definitions from ~/.aid/agents/*.toml.
// Exports: load_custom_agents, resolve_custom_agent, list_custom_agents.
// Deps: crate::paths, super::custom.
#![allow(dead_code)]

use super::custom::{parse_config, CustomAgent, CustomAgentConfig};
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
    registry.get(name).map(|config| {
        Box::new(CustomAgent {
            config: config.clone(),
        }) as Box<dyn super::Agent>
    })
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
}
