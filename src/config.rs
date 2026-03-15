// Config loading for aid usage budgets and future prompt settings.
// Exports AidConfig plus load_config() from ~/.aid/config.toml.

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::paths;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AidConfig {
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
    pub evermemos: crate::evermemos::EverMemosConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SelectionConfig {
    #[serde(default)]
    pub budget_mode: bool,
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
    pub request_limit: Option<u32>,
    #[serde(default)]
    pub external_tasks: u32,
    #[serde(default)]
    pub external_tokens: i64,
    #[serde(default)]
    pub external_cost_usd: f64,
    #[serde(default)]
    pub external_requests: u32,
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
    60
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
    toml::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::AidConfig;

    #[test]
    fn parses_usage_budgets() {
        let config: AidConfig = toml::from_str(
            r#"
            [[usage.budget]]
            name = "codex-dev"
            agent = "codex"
            window = "24h"
            task_limit = 12
            token_limit = 500000
            cost_limit_usd = 10.0

            [[usage.budget]]
            name = "claude-code"
            plan = "pro"
            window = "5h"
            request_limit = 200
            external_requests = 120
            "#,
        )
        .unwrap();

        assert_eq!(config.usage.budgets.len(), 2);
        assert_eq!(config.usage.budgets[0].agent.as_deref(), Some("codex"));
        assert_eq!(config.usage.budgets[1].external_requests, 120);
    }

    #[test]
    fn background_config_defaults_to_sixty_minutes() {
        let config = AidConfig::default();

        assert_eq!(config.background.max_task_duration_mins, 60);
    }

    #[test]
    fn parses_background_max_task_duration_override() {
        let config: AidConfig = toml::from_str(
            r#"
            [background]
            max_task_duration_mins = 120
            "#,
        )
        .unwrap();

        assert_eq!(config.background.max_task_duration_mins, 120);
    }

    #[test]
    fn parses_webhook_config() {
        let config: AidConfig = toml::from_str(
            r#"
            [[webhook]]
            name = "slack-notify"
            url = "https://hooks.slack.com/services/test"
            on_done = true
            headers = [["Authorization", "Bearer token"]]
            "#,
        )
        .unwrap();

        assert_eq!(config.webhooks.len(), 1);
        assert_eq!(config.webhooks[0].name, "slack-notify");
        assert!(config.webhooks[0].on_done);
        assert!(!config.webhooks[0].on_failed);
        assert_eq!(
            config.webhooks[0].headers[0],
            ("Authorization".to_string(), "Bearer token".to_string())
        );
    }

    #[test]
    fn parses_selection_budget_mode() {
        let config: AidConfig = toml::from_str(
            r#"
            [selection]
            budget_mode = true
            "#,
        )
        .unwrap();

        assert!(config.selection.budget_mode);
    }
}
