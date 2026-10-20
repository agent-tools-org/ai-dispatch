// Team definitions loaded from ~/.aid/teams/*.toml.
// Exports: TeamConfig, load_teams, resolve_team, list_teams, teams_dir.
// Deps: serde, toml, std::fs, crate::support::paths.

use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::support::paths;

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
    /// Soft preference for `aid advise` scoring — NOT a hard filter.
    /// All agents remain available; these just get a scoring boost.
    #[serde(alias = "agents")]
    pub preferred_agents: Vec<String>,
    pub default_agent: Option<String>,
    #[serde(default)]
    pub overrides: HashMap<String, CapabilityOverrides>,
    /// Always-injected behavioral constraints — no relevance filtering.
    #[serde(default)]
    pub rules: Vec<String>,
    /// Team toolbox: named tools and auto-inject list.
    #[serde(default)]
    pub toolbox: TeamToolbox,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TeamToolbox {
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub auto_inject: Vec<String>,
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
                        aid_warn!("Failed to parse {}: {}", path.display(), err);
                    }
                },
                Err(err) => {
                    aid_warn!("Failed to read {}: {}", path.display(), err);
                }
            }
        }
    }
    teams
}

pub fn parse_team(toml_content: &str) -> anyhow::Result<TeamConfig> {
    let file: TeamFile = toml::from_str(toml_content)?;
    for name in file.team.preferred_agents.iter()
        .chain(file.team.default_agent.iter()).chain(file.team.overrides.keys()) {
        crate::agent::removed::reject(name)?;
    }
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

pub struct KnowledgeEntry {
    pub topic: String,
    pub path: Option<String>,
    pub description: String,
    pub content: Option<String>,
}

pub fn read_knowledge_entries(team_id: &str) -> Vec<KnowledgeEntry> {
    let index_path = knowledge_index(team_id);
    let raw = match fs::read_to_string(&index_path) {
        Ok(body) => body,
        Err(_) => return Vec::new(),
    };
    if raw.trim().is_empty() {
        return Vec::new();
    }
    let base = teams_dir().join(team_id);
    raw.lines()
        .filter_map(|line| parse_knowledge_line(line, &base))
        .collect()
}

pub(crate) fn parse_knowledge_line(line: &str, base_dir: &Path) -> Option<KnowledgeEntry> {
    let trimmed = line.trim();
    if !trimmed.starts_with('-') {
        return None;
    }
    let rest = trimmed[1..].trim_start();
    if !rest.starts_with('[') {
        return None;
    }
    let closing = rest.find(']')?;
    if closing <= 1 {
        return None;
    }
    let topic = rest[1..closing].trim().to_string();
    let mut remainder = rest[closing + 1..].trim_start();
    let mut path = None;
    if remainder.starts_with('(') {
        let end = remainder.find(')')?;
        let segment = remainder[1..end].trim().to_string();
        if !segment.is_empty() {
            path = Some(segment);
        }
        remainder = remainder[end + 1..].trim_start();
    }
    let description = remainder.split_once('—')?.1.trim();
    if description.is_empty() {
        return None;
    }
    let content = path.as_ref().and_then(|relative| {
        let target = base_dir.join(relative);
        fs::read_to_string(&target)
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|t| !t.is_empty())
    });
    Some(KnowledgeEntry {
        topic,
        path,
        description: description.to_string(),
        content,
    })
}

#[cfg(test)]
#[path = "team_tests.rs"]
mod tests;
