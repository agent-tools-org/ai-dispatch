// Permission-only settings overlay for helper-backed Claude routes.
// Disables disk setting sources so their env blocks cannot redirect provider credentials.
use super::{ApiRoute, RunOpts};
use anyhow::{Context, Result};
use serde_json::{Map, Value, json};
use std::path::PathBuf;

pub(super) fn route_settings(route: &ApiRoute, opts: &RunOpts) -> Result<Value> {
    let mut files = Vec::new();
    if let Some(home) = opts
        .env
        .as_ref()
        .and_then(|env| env.get("HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
    {
        files.push(home.join(".claude/settings.json"));
    }
    let dir = opts
        .dir
        .as_ref()
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(std::env::current_dir)?;
    let dir = if dir.is_absolute() {
        dir
    } else {
        std::env::current_dir()?.join(dir)
    };
    let mut ancestors: Vec<_> = dir.ancestors().collect();
    ancestors.reverse();
    for dir in ancestors {
        files.push(dir.join(".claude/settings.json"));
        files.push(dir.join(".claude/settings.local.json"));
    }
    let permissions = permission_overlay(files)?;
    let mut settings = json!({"apiKeyHelper": route.helper_command()?});
    if !permissions.is_empty() {
        settings["permissions"] = Value::Object(permissions);
    }
    Ok(settings)
}

fn permission_overlay(files: Vec<PathBuf>) -> Result<Map<String, Value>> {
    let mut permissions = Map::new();
    let mut seen = std::collections::HashSet::new();
    for file in files {
        if !seen.insert(file.clone()) {
            continue;
        }
        let bytes = match std::fs::read(&file) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error).context("Cannot read route permission settings"),
        };
        let settings: Value =
            serde_json::from_slice(&bytes).context("Cannot parse route permission settings")?;
        if let Some(fields) = settings.get("permissions").and_then(Value::as_object) {
            for (key, value) in fields {
                if let (Some(Value::Array(current)), Value::Array(extra)) = (permissions.get_mut(key), value)
                {
                    current.extend(extra.iter().cloned());
                } else {
                    permissions.insert(key.clone(), value.clone());
                }
            }
        }
    }
    Ok(permissions)
}
