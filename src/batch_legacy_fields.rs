// Batch legacy-field validation for renamed TOML keys.
// Exports: validate_legacy_field_renames() before serde parsing.
// Deps: anyhow, toml::Value, batch task labeling by name/index.

use anyhow::{Context, Result};
use std::path::Path;

pub(crate) fn validate_legacy_field_renames(content: &str, path: &Path) -> Result<()> {
    let raw: toml::Value = toml::from_str(content)
        .with_context(|| format!("failed to parse TOML in {}", path.display()))?;
    let Some(table) = raw.as_table() else {
        return Ok(());
    };
    if table
        .get("defaults")
        .and_then(toml::Value::as_table)
        .is_some_and(|defaults| defaults.contains_key("timeout"))
    {
        anyhow::bail!(
            "batch field `timeout` was renamed to `max_duration_mins`; update [defaults] in {}",
            path.display()
        );
    }
    let Some(tasks) = table
        .get("tasks")
        .or_else(|| table.get("task"))
        .and_then(toml::Value::as_array)
    else {
        return Ok(());
    };
    for (task_idx, task) in tasks.iter().enumerate() {
        let Some(task_table) = task.as_table() else {
            continue;
        };
        if !task_table.contains_key("timeout") {
            continue;
        }
        let task_label = task_table
            .get("name")
            .and_then(toml::Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("#{task_idx}"));
        anyhow::bail!(
            "batch field `timeout` was renamed to `max_duration_mins`; update task {} in {}",
            task_label,
            path.display()
        );
    }
    Ok(())
}
