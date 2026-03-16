// Project config parsing for .aid/project.toml and built-in profiles.
// Exports: ProjectConfig, ProjectBudget, ProjectAgents, detect_project, project_rules, project_knowledge_dir, read_project_knowledge.
// Deps: serde, toml, anyhow, std::{env, fs, path}, crate::team.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::{env, fs};

use crate::team::{self, KnowledgeEntry};

#[derive(Debug, Clone, Deserialize)]
struct ProjectFile {
    #[serde(rename = "project")]
    pub project: ProjectConfig,
}

#[allow(dead_code)] // All fields used via TOML deserialization; agents integration planned
#[derive(Debug, Clone, Deserialize)]
#[derive(Default)]
pub struct ProjectConfig {
    pub id: String,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub team: Option<String>,
    #[serde(default)]
    pub verify: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub rules: Vec<String>,
    #[serde(default)]
    pub budget: ProjectBudget,
    #[serde(default)]
    pub agents: ProjectAgents,
}


#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct ProjectBudget {
    #[serde(default)]
    pub window: Option<String>,
    #[serde(default)]
    pub cost_limit_usd: Option<f64>,
    #[serde(default)]
    pub token_limit: Option<u64>,
    #[serde(default)]
    pub prefer_budget: bool,
}


#[allow(dead_code)] // Schema fields — used when TOML is parsed, agent integration planned
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct ProjectAgents {
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub research: Option<String>,
    #[serde(default)]
    pub simple_edit: Option<String>,
}


pub fn detect_project() -> Option<ProjectConfig> {
    let git_root = find_git_root()?;
    let project_path = git_root.join(".aid").join("project.toml");
    if !project_path.is_file() {
        return None;
    }
    load_project(&project_path).ok()
}

pub fn load_project(path: &Path) -> Result<ProjectConfig> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let file: ProjectFile =
        toml::from_str(&contents).with_context(|| format!("Failed to parse {}", path.display()))?;
    let mut config = file.project;
    apply_profile(&mut config);
    Ok(config)
}

pub fn project_knowledge_dir(git_root: &Path) -> PathBuf {
    git_root.join(".aid").join("knowledge")
}

pub fn read_project_knowledge(git_root: &Path) -> Vec<KnowledgeEntry> {
    let knowledge_dir = project_knowledge_dir(git_root);
    let index_path = knowledge_dir.join("KNOWLEDGE.md");
    let raw = match fs::read_to_string(&index_path) {
        Ok(body) => body,
        Err(_) => return Vec::new(),
    };
    raw.lines()
        .filter_map(|line| team::parse_knowledge_line(line, &knowledge_dir))
        .collect()
}

fn find_git_root() -> Option<PathBuf> {
    let mut dir = env::current_dir().ok()?;
    loop {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

fn apply_profile(config: &mut ProjectConfig) {
    let profile = config.profile.as_deref().map(str::to_lowercase);
    let profile = match profile {
        Some(ref value) => value.as_str(),
        None => return,
    };

    match profile {
        "hobby" => apply_hobby_profile(config),
        "standard" => apply_standard_profile(config),
        "production" => apply_production_profile(config),
        _ => {}
    }
}

fn apply_hobby_profile(config: &mut ProjectConfig) {
    if config.budget.cost_limit_usd.is_none() {
        config.budget.cost_limit_usd = Some(5.0);
    }
    config.budget.prefer_budget = true;
}

fn apply_standard_profile(config: &mut ProjectConfig) {
    if config.verify.is_none() {
        config.verify = Some("auto".to_string());
    }
    if config.budget.cost_limit_usd.is_none() {
        config.budget.cost_limit_usd = Some(20.0);
    }
    append_rule(
        &mut config.rules,
        "All new functions must have at least one test",
    );
    config.budget.prefer_budget = false;
}

fn apply_production_profile(config: &mut ProjectConfig) {
    if config.verify.is_none() {
        config.verify = Some(default_production_verify(config));
    }
    if config.budget.cost_limit_usd.is_none() {
        config.budget.cost_limit_usd = Some(50.0);
    }
    append_rule(&mut config.rules, "All changes must have tests");
    append_rule(&mut config.rules, "No unwrap() in production code");
    append_rule(&mut config.rules, "Changes require cross-review");
    config.budget.prefer_budget = false;
}

fn default_production_verify(config: &ProjectConfig) -> String {
    let language = config.language.as_deref().unwrap_or("").to_lowercase();
    if language == "typescript" || language == "javascript" || language == "node" {
        "npm test".to_string()
    } else {
        "cargo test".to_string()
    }
}

fn append_rule(rules: &mut Vec<String>, rule: &str) {
    if !rules.iter().any(|existing| existing == rule) {
        rules.push(rule.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn write_project(dir: &Path, contents: &str) -> PathBuf {
        let path = dir.join("project.toml");
        fs::write(&path, contents).unwrap();
        path
    }

    struct TempCwd {
        previous: PathBuf,
    }

    impl TempCwd {
        fn enter(target: &Path) -> Self {
            let previous = env::current_dir().unwrap();
            env::set_current_dir(target).unwrap();
            Self { previous }
        }
    }

    impl Drop for TempCwd {
        fn drop(&mut self) {
            env::set_current_dir(&self.previous).unwrap();
        }
    }

    #[test]
    fn parses_minimal_toml() {
        let dir = TempDir::new().unwrap();
        let contents = r#"[project]
id = "alpha"
"#;
        let config = load_project(&write_project(dir.path(), contents)).unwrap();
        assert_eq!(config.id, "alpha");
        assert!(config.rules.is_empty());
    }

    #[test]
    fn profile_expands_standard_defaults() {
        let dir = TempDir::new().unwrap();
        let contents = r#"[project]
id = "beta"
profile = "standard"
"#;
        let config = load_project(&write_project(dir.path(), contents)).unwrap();
        assert_eq!(config.verify.as_deref(), Some("auto"));
        assert_eq!(config.budget.cost_limit_usd, Some(20.0));
        assert!(config
            .rules
            .iter()
            .any(|rule| rule.contains("new functions")));
    }

    #[test]
    fn profile_defaults_respect_explicit_values() {
        let dir = TempDir::new().unwrap();
        let contents = r#"[project]
id = "gamma"
profile = "standard"
verify = "custom verify"
rules = ["explicit rule"]
budget.window = "4h"
budget.cost_limit_usd = 99.5
"#;
        let config = load_project(&write_project(dir.path(), contents)).unwrap();
        assert_eq!(config.verify.as_deref(), Some("custom verify"));
        assert!(config.rules.iter().any(|rule| rule == "explicit rule"));
        assert!(config
            .rules
            .iter()
            .any(|rule| rule.contains("new functions")));
        assert_eq!(config.budget.cost_limit_usd, Some(99.5));
    }

    #[test]
    fn detect_project_returns_none_outside_git() {
        let dir = TempDir::new().unwrap();
        let _guard = TempCwd::enter(dir.path());
        assert!(detect_project().is_none());
    }

    #[test]
    fn test_read_project_knowledge() {
        let dir = TempDir::new().unwrap();
        let git_root = dir.path();
        fs::create_dir_all(git_root.join(".git")).unwrap();
        let knowledge_dir = project_knowledge_dir(git_root);
        fs::create_dir_all(&knowledge_dir).unwrap();
        fs::write(
            knowledge_dir.join("KNOWLEDGE.md"),
            "- [Guide](guide.md) — Useful knowledge\n- [Note] — Standalone note\n",
        )
        .unwrap();
        fs::write(knowledge_dir.join("guide.md"), "Details\n").unwrap();

        let entries = read_project_knowledge(git_root);
        assert_eq!(entries.len(), 2);
        let guide = entries
            .into_iter()
            .find(|entry| entry.topic == "Guide")
            .unwrap();
        assert_eq!(guide.path.as_deref(), Some("guide.md"));
        assert_eq!(guide.description, "Useful knowledge");
        assert_eq!(guide.content.as_deref(), Some("Details"));
    }
}
