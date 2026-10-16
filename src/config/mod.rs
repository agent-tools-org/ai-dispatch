// Config loading for aid usage budgets and future prompt settings.
// Exports AidConfig plus load_config() from ~/.aid/config.toml.

pub(crate) mod agent;
pub(crate) mod api_route;
pub(crate) mod project;
pub(crate) mod team;
pub(crate) mod toolbox;
pub(crate) mod hooks;
pub(crate) mod timeout_policy;
pub(crate) mod idle_timeout;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::support::paths;

pub(crate) const DEFAULT_MAX_TASK_DURATION_MINS: i64 =
    crate::config::timeout_policy::DEFAULT_MAX_DURATION_MINS;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AidConfig {
    #[serde(default)]
    pub(crate) api_route: Vec<api_route::ApiRoute>,
    #[serde(default)]
    pub usage: UsageConfig,
    #[serde(default)]
    pub background: BackgroundConfig,
    #[serde(default)]
    pub selection: SelectionConfig,
    #[serde(default, rename = "webhook")]
    pub webhooks: Vec<WebhookConfig>,
    #[serde(default)]
    pub query: QueryConfig,
    #[serde(default)]
    pub updates: UpdateConfig,
    #[serde(default)]
    pub backup: crate::workspace::backup::BackupGlobalConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateConfig {
    #[serde(default = "default_check_updates")]
    pub check: bool,
}

fn default_check_updates() -> bool {
    true
}

fn default_true() -> bool {
    true
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            check: default_check_updates(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SelectionConfig {
    #[serde(default)]
    pub budget_mode: bool,
    #[serde(default = "default_true")]
    pub smart_routing: bool,
}

impl Default for SelectionConfig {
    fn default() -> Self {
        Self {
            budget_mode: false,
            smart_routing: default_true(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UsageConfig {
    #[serde(default, rename = "budget")]
    pub budgets: Vec<UsageBudget>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UsageBudget {
    pub name: String,
    pub plan: Option<String>,
    pub agent: Option<String>,
    pub window: Option<String>,
    pub task_limit: Option<u32>,
    pub token_limit: Option<i64>,
    pub cost_limit_usd: Option<f64>,
    #[serde(default)]
    pub external_tasks: u32,
    #[serde(default)]
    pub external_tokens: i64,
    #[serde(default)]
    pub external_cost_usd: f64,
    pub resets_at: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BackgroundConfig {
    #[serde(default = "default_max_duration")]
    pub max_task_duration_mins: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebhookConfig {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub on_done: bool,
    #[serde(default)]
    pub on_failed: bool,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QueryConfig {
    /// Free-tier model (default)
    #[serde(default = "default_free_model")]
    pub free_model: String,
    /// Auto-tier model (--auto flag)
    #[serde(default = "default_auto_model")]
    pub auto_model: String,
    /// OpenRouter API key (overrides OPENROUTER_API_KEY env var)
    pub api_key: Option<String>,
}

fn default_free_model() -> String {
    "openrouter/free".to_string()
}

fn default_auto_model() -> String {
    "openrouter/auto".to_string()
}

impl Default for QueryConfig {
    fn default() -> Self {
        Self {
            free_model: default_free_model(),
            auto_model: default_auto_model(),
            api_key: None,
        }
    }
}

fn default_max_duration() -> i64 {
    DEFAULT_MAX_TASK_DURATION_MINS
}

impl Default for BackgroundConfig {
    fn default() -> Self {
        Self {
            max_task_duration_mins: default_max_duration(),
        }
    }
}

pub fn load_config() -> Result<AidConfig> {
    let path = paths::config_path();
    if !path.exists() {
        return Ok(AidConfig::default());
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    parse_config(&content).with_context(|| format!("Failed to parse {}", path.display()))
}

pub(crate) fn load_dispatch_config() -> Result<AidConfig> {
    let path = paths::config_path();
    if !path.exists() { return Ok(AidConfig::default()); }
    let content = std::fs::read_to_string(&path)?;
    let mut value: toml::Value = toml::from_str(&content)?;
    if let Some(table) = value.as_table_mut() { table.remove("api_route"); }
    value.try_into().context("Invalid dispatch configuration")
}

pub(crate) fn parse_config(content: &str) -> Result<AidConfig> {
    let config: AidConfig = toml::from_str(content)?;
    api_route::validate_routes(&config.api_route)?;
    Ok(config)
}

mod budgets;
pub use budgets::{effective_budget, upsert_budget, upsert_budget_limits};

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
