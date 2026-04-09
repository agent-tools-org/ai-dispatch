// Config loading for aid usage budgets and future prompt settings.
// Exports AidConfig plus load_config() from ~/.aid/config.toml.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::{fs, path::Path};
use toml::value::{Table, Value};

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
    pub updates: UpdateConfig,
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

/// Update or create a `[[usage.budget]]` entry in the global config file.
/// If an entry with the same `name` already exists, update its cost_limit_usd and window.
/// If not, append a new entry.
pub fn upsert_budget(name: &str, cost_limit_usd: f64, window: Option<&str>) -> Result<()> {
    upsert_budget_at(&paths::config_path(), name, cost_limit_usd, window)
}

fn upsert_budget_at(path: &Path, name: &str, cost_limit_usd: f64, window: Option<&str>) -> Result<()> {
    let mut document = if path.exists() {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        toml::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))?
    } else {
        Value::Table(Table::new())
    };

    let root = ensure_table(&mut document);
    let usage_value = root.entry("usage").or_insert_with(|| Value::Table(Table::new()));
    let usage = ensure_table(usage_value);
    let budget_value = usage.entry("budget").or_insert_with(|| Value::Array(Vec::new()));
    let budgets = ensure_array(budget_value);

    let mut updated = false;
    for entry in budgets.iter_mut() {
        if let Value::Table(table) = entry
            && table.get("name").and_then(|n| n.as_str()) == Some(name)
        {
            table.insert("cost_limit_usd".to_string(), Value::Float(cost_limit_usd));
            if let Some(window) = window {
                table.insert("window".to_string(), Value::String(window.to_string()));
            } else {
                table.remove("window");
            }
            updated = true;
            break;
        }
    }

    if !updated {
        let mut entry = Table::new();
        entry.insert("name".to_string(), Value::String(name.to_string()));
        entry.insert("cost_limit_usd".to_string(), Value::Float(cost_limit_usd));
        if let Some(window) = window {
            entry.insert("window".to_string(), Value::String(window.to_string()));
        }
        budgets.push(Value::Table(entry));
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    let serialized = toml::to_string_pretty(&document).context("Failed to serialize config file")?;
    fs::write(path, serialized)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

/// Read the effective budget for a given project name from the global config.
/// Returns (cost_limit_usd, window) if found.
pub fn effective_budget(name: &str) -> Result<Option<(f64, Option<String>)>> {
    effective_budget_at(&paths::config_path(), name)
}

fn effective_budget_at(path: &Path, name: &str) -> Result<Option<(f64, Option<String>)>> {
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let document: Value = toml::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;

    Ok(find_budget(&document, name))
}

fn find_budget(value: &Value, name: &str) -> Option<(f64, Option<String>)> {
    let usage = value.get("usage")?.as_table()?;
    let budgets = usage.get("budget")?.as_array()?;
    for entry in budgets {
        if let Value::Table(table) = entry
            && table.get("name")?.as_str()? == name
        {
            let cost = table.get("cost_limit_usd")?;
            let cost = value_to_f64(cost)?;
            let window = table
                .get("window")
                .and_then(|w| w.as_str())
                .map(|s| s.to_string());
            return Some((cost, window));
        }
    }
    None
}

fn ensure_table(value: &mut Value) -> &mut Table {
    if !value.is_table() {
        *value = Value::Table(Table::new());
    }
    value.as_table_mut().expect("value is table")
}

fn ensure_array(value: &mut Value) -> &mut Vec<Value> {
    if !value.is_array() {
        *value = Value::Array(Vec::new());
    }
    value.as_array_mut().expect("value is array")
}

fn value_to_f64(value: &Value) -> Option<f64> {
    value
        .as_float()
        .or_else(|| value.as_integer().map(|i| i as f64))
}

#[cfg(test)]
mod tests {
    use super::{AidConfig, effective_budget_at, upsert_budget_at};
    use std::fs;
    use tempfile::TempDir;
    use toml::Value;

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
            name = "gemini-daily"
            agent = "gemini"
            window = "24h"
            task_limit = 50
            cost_limit_usd = 5.0
            "#,
        )
        .unwrap();

        assert_eq!(config.usage.budgets.len(), 2);
        assert_eq!(config.usage.budgets[0].agent.as_deref(), Some("codex"));
        assert_eq!(config.usage.budgets[1].agent.as_deref(), Some("gemini"));
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
        assert!(config.selection.smart_routing);
    }

    #[test]
    fn selection_smart_routing_defaults_to_true() {
        assert!(AidConfig::default().selection.smart_routing);

        let config: AidConfig = toml::from_str("[selection]").unwrap();
        assert!(config.selection.smart_routing);
    }

    #[test]
    fn updates_check_defaults_to_true_and_allows_override() {
        assert!(AidConfig::default().updates.check);

        let config: AidConfig = toml::from_str(
            r#"
            [updates]
            check = false
            "#,
        )
        .unwrap();

        assert!(!config.updates.check);
    }

    #[test]
    fn upsert_creates_new_entry() -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        let path = dir.path().join("config.toml");

        upsert_budget_at(&path, "project", 12.5, Some("24h"))?;

        let content = fs::read_to_string(&path)?;
        let parsed: Value = toml::from_str(&content)?;
        let budgets = parsed
            .get("usage")
            .and_then(|u| u.get("budget"))
            .and_then(|b| b.as_array())
            .expect("budget array missing");

        assert_eq!(budgets.len(), 1);
        let entry = budgets[0].as_table().unwrap();
        assert_eq!(entry.get("name").and_then(|n| n.as_str()), Some("project"));
        assert_eq!(entry.get("cost_limit_usd").and_then(|c| c.as_float()), Some(12.5));
        assert_eq!(entry.get("window").and_then(|w| w.as_str()), Some("24h"));
        Ok(())
    }

    #[test]
    fn upsert_updates_existing() -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
            [[usage.budget]]
            name = "test"
            cost_limit_usd = 5.0
            window = "2h"
            "#,
        )?;

        upsert_budget_at(&path, "test", 20.0, Some("10h"))?;

        let parsed: Value = toml::from_str(&fs::read_to_string(&path)?)?;
        let budgets = parsed
            .get("usage")
            .and_then(|u| u.get("budget"))
            .and_then(|b| b.as_array())
            .unwrap();
        let entry = budgets[0].as_table().unwrap();
        assert_eq!(entry.get("cost_limit_usd").and_then(|c| c.as_float()), Some(20.0));
        assert_eq!(entry.get("window").and_then(|w| w.as_str()), Some("10h"));
        Ok(())
    }

    #[test]
    fn upsert_preserves_other_config() -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
            [query]
            api_key = "sk-xxx"

            [[usage.budget]]
            name = "snapshot"
            cost_limit_usd = 3
            "#,
        )?;

        upsert_budget_at(&path, "snapshot", 4.5, None)?;

        let parsed: Value = toml::from_str(&fs::read_to_string(&path)?)?;
        assert_eq!(parsed.get("query").and_then(|q| q.get("api_key")).and_then(|k| k.as_str()), Some("sk-xxx"));
        Ok(())
    }

    #[test]
    fn upsert_adds_second_budget() -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
            [[usage.budget]]
            name = "alpha"
            cost_limit_usd = 1
            "#,
        )?;

        upsert_budget_at(&path, "beta", 2.0, None)?;

        let parsed: Value = toml::from_str(&fs::read_to_string(&path)?)?;
        let budgets = parsed
            .get("usage")
            .and_then(|u| u.get("budget"))
            .and_then(|b| b.as_array())
            .unwrap();
        assert_eq!(budgets.len(), 2);
        assert!(budgets.iter().any(|entry| {
            entry
                .as_table()
                .and_then(|table| table.get("name"))
                .and_then(|n| n.as_str())
                == Some("beta")
        }));
        Ok(())
    }

    #[test]
    fn effective_budget_found() -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
            [[usage.budget]]
            name = "target"
            cost_limit_usd = 7.25
            window = "weekly"
            "#,
        )?;

        let budget = effective_budget_at(&path, "target")?;
        assert_eq!(budget, Some((7.25, Some("weekly".to_string()))));
        Ok(())
    }

    #[test]
    fn effective_budget_not_found() -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
            [[usage.budget]]
            name = "other"
            cost_limit_usd = 1
            "#,
        )?;

        let budget = effective_budget_at(&path, "missing")?;
        assert!(budget.is_none());
        Ok(())
    }
}
