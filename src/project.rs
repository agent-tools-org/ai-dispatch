// Project config parsing for .aid/project.toml and built-in profiles.
// Exports: ProjectConfig, ProjectBudget, ProjectAgents, detect_project, knowledge helpers.
// Deps: serde, toml, anyhow, std::{env, fs, path}, and project submodules.

use anyhow::{Context, Result};
use serde::de::{Deserializer, IntoDeserializer};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::{env, fs};

#[path = "project/audit.rs"]
mod audit;
#[path = "project/team.rs"]
mod project_team;

use self::audit::ProjectFile;
pub use self::audit::ProjectAuditConfig;
pub use self::project_team::{project_knowledge_dir, read_project_knowledge};

#[derive(Debug, Clone, Deserialize)]
#[derive(Default)]
pub struct ProjectConfig {
    pub id: String,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub max_task_cost: Option<f64>,
    #[serde(default)]
    pub team: Option<String>,
    #[serde(default)]
    pub verify: Option<String>,
    #[serde(default)]
    pub setup: Option<String>,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub gitbutler: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub rules: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_budget")]
    pub budget: ProjectBudget,
    #[serde(default)]
    pub agents: ProjectAgents,
    #[serde(skip)]
    pub audit: ProjectAuditConfig,
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

impl ProjectBudget {
    pub fn budget_shorthand(&self) -> Option<String> {
        let cost = self.cost_limit_usd?;
        let mut shorthand = format!("${}", cost);
        if let Some(window) = self.window.as_deref() {
            match window.to_lowercase().as_str() {
                "day" | "daily" => shorthand.push_str("/day"),
                "month" | "monthly" => shorthand.push_str("/month"),
                other => {
                    shorthand.push('/');
                    shorthand.push_str(other);
                }
            }
        }
        Some(shorthand)
    }
}

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

impl ProjectConfig {
    pub fn audit_auto(&self) -> bool {
        self.audit.auto
    }

    pub fn gitbutler_mode(&self) -> crate::gitbutler::Mode {
        let Some(value) = self.gitbutler.as_deref() else {
            return crate::gitbutler::Mode::Off;
        };

        match crate::gitbutler::Mode::from_str(value) {
            Ok(mode) => mode,
            Err(err) => {
                aid_warn!(
                    "[aid] Warning: invalid project.gitbutler mode '{value}': {err}. Falling back to off."
                );
                crate::gitbutler::Mode::Off
            }
        }
    }
}

fn deserialize_budget<'de, D>(deserializer: D) -> Result<ProjectBudget, D::Error>
where
    D: Deserializer<'de>,
{
    let value = toml::Value::deserialize(deserializer)?;
    match value {
        toml::Value::String(raw) => parse_budget_shorthand(&raw).map_err(serde::de::Error::custom),
        toml::Value::Integer(amount) => Ok(ProjectBudget {
            cost_limit_usd: Some(amount as f64),
            ..Default::default()
        }),
        toml::Value::Float(amount) => Ok(ProjectBudget {
            cost_limit_usd: Some(amount),
            ..Default::default()
        }),
        toml::Value::Table(table) => ProjectBudget::deserialize(
            toml::Value::Table(table).into_deserializer(),
        )
        .map_err(serde::de::Error::custom),
        other => Err(serde::de::Error::custom(format!(
            "invalid budget value: {other:?}"
        ))),
    }
}

fn parse_budget_shorthand(value: &str) -> Result<ProjectBudget, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("budget shorthand is empty".to_string());
    }
    let trimmed = trimmed.strip_prefix('$').unwrap_or(trimmed).trim();
    if trimmed.is_empty() {
        return Err("budget amount is missing".to_string());
    }

    let (amount_part, window_part) = match trimmed.split_once('/') {
        Some((left, right)) => (left.trim(), Some(right.trim())),
        None => (trimmed, None),
    };

    if amount_part.is_empty() {
        return Err("budget amount is missing".to_string());
    }

    let cost_limit_usd = amount_part
        .parse::<f64>()
        .map_err(|_| format!("invalid budget amount '{}'", amount_part))?;

    let window = match window_part {
        Some(w) if !w.is_empty() => match w.to_lowercase().as_str() {
            "day" | "daily" => Some("daily".to_string()),
            "month" | "monthly" => Some("monthly".to_string()),
            other => {
                return Err(format!("unsupported budget window '{}'", other));
            }
        },
        Some(_) => return Err("budget window is empty".to_string()),
        None => None,
    };

    Ok(ProjectBudget {
        cost_limit_usd: Some(cost_limit_usd),
        window,
        ..Default::default()
    })
}


pub fn detect_project() -> Option<ProjectConfig> {
    let cwd = env::current_dir().ok()?;
    detect_project_in(&cwd)
}

pub fn detect_project_in(start_dir: &Path) -> Option<ProjectConfig> {
    let git_root = find_git_root_from(start_dir)?;
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
    config.audit = file.audit;
    apply_profile(&mut config);
    Ok(config)
}
fn find_git_root_from(start_dir: &Path) -> Option<PathBuf> {
    let mut dir = start_dir.to_path_buf();
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
    if config.max_task_cost.is_none() {
        config.max_task_cost = Some(2.0);
    }
    if config.budget.cost_limit_usd.is_none() {
        config.budget.cost_limit_usd = Some(5.0);
    }
    config.budget.prefer_budget = true;
}

fn apply_standard_profile(config: &mut ProjectConfig) {
    if config.max_task_cost.is_none() {
        config.max_task_cost = Some(10.0);
    }
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
    if config.max_task_cost.is_none() {
        config.max_task_cost = Some(25.0);
    }
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
#[path = "project/tests.rs"]
mod tests;
