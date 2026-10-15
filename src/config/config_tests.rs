// Global configuration and budget editing regression tests.
// Uses temporary TOML files and synthetic usage limits.
use super::AidConfig;
use super::budgets::{effective_budget_at, upsert_budget_at};
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

    assert_eq!(
        config.background.max_task_duration_mins,
        crate::config::DEFAULT_MAX_TASK_DURATION_MINS
    );
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

    upsert_budget_at(&path, "project", Some(12.5), None, Some("24h"))?;

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

    upsert_budget_at(&path, "test", Some(20.0), None, Some("10h"))?;

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
fn upsert_writes_token_limit() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let path = dir.path().join("config.toml");

    upsert_budget_at(&path, "tokenized", Some(3.0), Some(12_000), Some("24h"))?;

    let parsed: Value = toml::from_str(&fs::read_to_string(&path)?)?;
    let budgets = parsed
        .get("usage")
        .and_then(|u| u.get("budget"))
        .and_then(|b| b.as_array())
        .unwrap();
    let entry = budgets[0].as_table().unwrap();
    assert_eq!(entry.get("cost_limit_usd").and_then(|c| c.as_float()), Some(3.0));
    assert_eq!(
        entry.get("token_limit").and_then(|t| t.as_integer()),
        Some(12_000)
    );
    assert_eq!(entry.get("window").and_then(|w| w.as_str()), Some("24h"));
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

    upsert_budget_at(&path, "snapshot", Some(4.5), None, None)?;

    let parsed: Value = toml::from_str(&fs::read_to_string(&path)?)?;
    assert_eq!(
        parsed
            .get("query")
            .and_then(|q| q.get("api_key"))
            .and_then(|k| k.as_str()),
        Some("sk-xxx")
    );
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

    upsert_budget_at(&path, "beta", Some(2.0), None, None)?;

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
