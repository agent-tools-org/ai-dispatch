// Team definitions loaded from ~/.aid/teams/*.toml.
// Exports: TeamConfig, load_teams, resolve_team, list_teams, teams_dir.
// Deps: serde, toml, std::fs, crate::paths.

use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::paths;

#[derive(Debug, Clone, Deserialize)]
pub struct TeamFile {
    pub team: TeamConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TeamConfig {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    /// Soft preference for auto-selection — NOT a hard filter.
    /// All agents remain available; these just get a scoring boost.
    #[serde(alias = "agents")]
    pub preferred_agents: Vec<String>,
    pub default_agent: Option<String>,
    #[serde(default)]
    pub overrides: HashMap<String, CapabilityOverrides>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CapabilityOverrides {
    #[serde(default)]
    pub research: Option<i32>,
    #[serde(default)]
    pub simple_edit: Option<i32>,
    #[serde(default)]
    pub complex_impl: Option<i32>,
    #[serde(default)]
    pub frontend: Option<i32>,
    #[serde(default)]
    pub debugging: Option<i32>,
    #[serde(default)]
    pub testing: Option<i32>,
    #[serde(default)]
    pub refactoring: Option<i32>,
    #[serde(default)]
    pub documentation: Option<i32>,
}

pub fn teams_dir() -> PathBuf {
    paths::aid_dir().join("teams")
}

fn load_from_dir(dir: &PathBuf) -> HashMap<String, TeamConfig> {
    let mut teams = HashMap::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }
            match fs::read_to_string(&path) {
                Ok(contents) => match parse_team(&contents) {
                    Ok(config) => {
                        let id = config.id.clone();
                        teams.insert(id, config);
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
    teams
}

pub fn parse_team(toml_content: &str) -> anyhow::Result<TeamConfig> {
    let file: TeamFile = toml::from_str(toml_content)?;
    Ok(file.team)
}

pub fn load_teams() -> HashMap<String, TeamConfig> {
    load_from_dir(&teams_dir())
}

pub fn resolve_team(name: &str) -> Option<TeamConfig> {
    load_teams().remove(name)
}

pub fn list_teams() -> Vec<TeamConfig> {
    let registry = load_teams();
    let mut teams: Vec<_> = registry.into_values().collect();
    teams.sort_by(|a, b| a.id.cmp(&b.id));
    teams
}

pub fn team_exists(name: &str) -> bool {
    teams_dir().join(format!("{name}.toml")).is_file() || load_teams().contains_key(name)
}

/// Directory for team-specific knowledge files.
pub fn knowledge_dir(team_id: &str) -> PathBuf {
    teams_dir().join(team_id).join("knowledge")
}

/// Path to team KNOWLEDGE.md index file.
pub fn knowledge_index(team_id: &str) -> PathBuf {
    teams_dir().join(team_id).join("KNOWLEDGE.md")
}

/// Read team knowledge index content (returns None if missing or empty).
pub fn read_knowledge(team_id: &str) -> Option<String> {
    let path = knowledge_index(team_id);
    let content = fs::read_to_string(path).ok()?;
    if content.trim().is_empty() { return None; }
    Some(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    fn write_team(dir: &Path, file: &str, contents: &str) {
        fs::write(dir.join(file), contents).unwrap();
    }

    fn sample_team_toml(id: &str) -> String {
        format!(
            r#"[team]
id = "{id}"
display_name = "{id} team"
preferred_agents = ["codex", "opencode"]
"#,
        )
    }

    #[test]
    fn empty_dir_returns_empty() {
        let dir = TempDir::new().unwrap();
        assert!(load_from_dir(&dir.path().to_path_buf()).is_empty());
    }

    #[test]
    fn loads_valid_toml() {
        let dir = TempDir::new().unwrap();
        write_team(dir.path(), "dev.toml", &sample_team_toml("dev"));
        let map = load_from_dir(&dir.path().to_path_buf());
        assert!(map.contains_key("dev"));
        assert_eq!(map["dev"].preferred_agents, vec!["codex", "opencode"]);
    }

    #[test]
    fn skips_invalid_toml() {
        let dir = TempDir::new().unwrap();
        write_team(dir.path(), "bad.toml", "not = valid = toml");
        assert!(load_from_dir(&dir.path().to_path_buf()).is_empty());
    }

    #[test]
    fn parses_full_team_with_overrides() {
        let toml_data = r#"
            [team]
            id = "dev"
            display_name = "Development Team"
            description = "Feature development"
            preferred_agents = ["codex", "opencode", "kilo"]
            default_agent = "codex"

            [team.overrides.opencode]
            simple_edit = 10
            debugging = 6

            [team.overrides.kilo]
            simple_edit = 9
        "#;
        let config = parse_team(toml_data).unwrap();
        assert_eq!(config.id, "dev");
        assert_eq!(config.preferred_agents.len(), 3);
        assert_eq!(config.default_agent, Some("codex".to_string()));
        assert_eq!(config.overrides.len(), 2);
        assert_eq!(config.overrides["opencode"].simple_edit, Some(10));
        assert_eq!(config.overrides["kilo"].simple_edit, Some(9));
    }

    #[test]
    fn list_returns_sorted() {
        let dir = TempDir::new().unwrap();
        write_team(dir.path(), "b.toml", &sample_team_toml("b"));
        write_team(dir.path(), "a.toml", &sample_team_toml("a"));
        let map = load_from_dir(&dir.path().to_path_buf());
        let mut teams: Vec<_> = map.into_values().collect();
        teams.sort_by(|a, b| a.id.cmp(&b.id));
        let ids: Vec<_> = teams.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
    }
}
