// Global usage-budget editing and lookup helpers.
// Preserves unrelated TOML configuration while updating named budgets.
use crate::support::paths;
use anyhow::{Context, Result};
use std::{fs, path::Path};
use toml::value::{Table, Value};

/// Update or create a `[[usage.budget]]` entry in the global config file.
/// If an entry with the same `name` already exists, update its limits and window.
/// If not, append a new entry.
pub fn upsert_budget(name: &str, cost_limit_usd: f64, window: Option<&str>) -> Result<()> {
    upsert_budget_at(&paths::config_path(), name, Some(cost_limit_usd), None, window)
}

pub fn upsert_budget_limits(
    name: &str,
    cost_limit_usd: Option<f64>,
    token_limit: Option<u64>,
    window: Option<&str>,
) -> Result<()> {
    upsert_budget_at(&paths::config_path(), name, cost_limit_usd, token_limit, window)
}

pub(super) fn upsert_budget_at(
    path: &Path,
    name: &str,
    cost_limit_usd: Option<f64>,
    token_limit: Option<u64>,
    window: Option<&str>,
) -> Result<()> {
    let mut document = if path.exists() {
        let content =
            fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
        toml::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))?
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
            apply_budget_limits(table, cost_limit_usd, token_limit)?;
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
        apply_budget_limits(&mut entry, cost_limit_usd, token_limit)?;
        if let Some(window) = window {
            entry.insert("window".to_string(), Value::String(window.to_string()));
        }
        budgets.push(Value::Table(entry));
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    let serialized = toml::to_string_pretty(&document).context("Failed to serialize config file")?;
    fs::write(path, serialized).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

fn apply_budget_limits(
    table: &mut Table,
    cost_limit_usd: Option<f64>,
    token_limit: Option<u64>,
) -> Result<()> {
    if let Some(cost_limit_usd) = cost_limit_usd {
        table.insert("cost_limit_usd".to_string(), Value::Float(cost_limit_usd));
    } else {
        table.remove("cost_limit_usd");
    }
    if let Some(token_limit) = token_limit {
        let token_limit = i64::try_from(token_limit).context("token_limit exceeds TOML integer range")?;
        table.insert("token_limit".to_string(), Value::Integer(token_limit));
    } else {
        table.remove("token_limit");
    }
    Ok(())
}

/// Read the effective budget for a given project name from the global config.
/// Returns (cost_limit_usd, window) if found.
pub fn effective_budget(name: &str) -> Result<Option<(f64, Option<String>)>> {
    effective_budget_at(&paths::config_path(), name)
}

pub(super) fn effective_budget_at(path: &Path, name: &str) -> Result<Option<(f64, Option<String>)>> {
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let document: Value =
        toml::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))?;

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
    value.as_float().or_else(|| value.as_integer().map(|i| i as f64))
}
