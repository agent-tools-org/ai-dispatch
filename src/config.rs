// Config loading for aid usage budgets and future prompt settings.
// Exports AidConfig plus load_config() from ~/.aid/config.toml.

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::paths;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AidConfig {
    #[serde(default)]
    pub usage: UsageConfig,
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
}
