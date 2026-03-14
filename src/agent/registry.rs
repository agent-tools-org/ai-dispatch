// Custom agent registry: loads agent definitions from ~/.aid/agents/*.toml.
// Exports: load_custom_agents, resolve_custom_agent, list_custom_agents.
// Deps: crate::paths, super::custom.
#![allow(dead_code)]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::custom::{CustomAgent, CustomAgentConfig, parse_config};
use crate::paths;

static REGISTRY: OnceLock<HashMap<String, CustomAgentConfig>> = OnceLock::new();

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
                        eprintln!("Failed to parse {}: {}", path.display(), err);
                    }
                },
                Err(err) => {
                    eprintln!("Failed to read {}: {}", path.display(), err);
                }
            }
        }
    }
    agents
}

fn get_registry() -> &'static HashMap<String, CustomAgentConfig> {
    REGISTRY.get_or_init(|| load_from_dir(&agents_dir()))
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
    resolve_from_registry(get_registry(), name)
}

fn list_from_registry(registry: &HashMap<String, CustomAgentConfig>) -> Vec<CustomAgentConfig> {
    let mut agents: Vec<_> = registry.values().cloned().collect();
    agents.sort_by(|a, b| a.id.cmp(&b.id));
    agents
}

pub fn list_custom_agents() -> Vec<CustomAgentConfig> {
    list_from_registry(get_registry())
}

pub fn custom_agent_exists(name: &str) -> bool {
    agents_dir().join(format!("{name}.toml")).is_file() || get_registry().contains_key(name)
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
            r#"
id = "{id}"
display_name = "{id} agent"
command = ["{id}"]
model = "gpt-4.1"
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
        let ids: Vec<_> = list_from_registry(&map)
            .iter()
            .map(|config| config.id.as_str())
            .collect();
        assert_eq!(ids, vec!["a", "b"]);
    }
}
