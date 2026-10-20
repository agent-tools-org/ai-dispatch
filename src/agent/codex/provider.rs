// Carry only the selected Codex provider through isolated launch configuration.
// Validates values before command construction; credentials remain environment references.

use anyhow::{Context, Result, ensure};
use std::process::Command;
#[path = "provider_validation.rs"]
mod validation;
use validation::inline_credentials;
pub(crate) use validation::safe_setting;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Provider {
    pub id: String,
    pub settings: toml::Table,
}

pub(crate) fn selected() -> Result<Option<Provider>> {
    let config = read_config()?;
    parse_config(&config)
}

fn read_config() -> Result<toml::Table> {
    let path = super::cli_config::codex_home()?.join("config.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(toml::Table::new()),
        Err(_) => anyhow::bail!(
            "Cannot read Codex provider configuration: {}",
            path.display()
        ),
    };
    match toml::from_str(&text) {
        Ok(config) => Ok(config),
        Err(_) if provider_declared(&text) => anyhow::bail!(
            "Parse error in Codex provider configuration: {}",
            path.display()
        ),
        Err(_) => {
            eprintln!(
                "Warning: parse error in {}; no provider determined, using default Codex provider",
                path.display()
            );
            Ok(toml::Table::new())
        }
    }
}

// Inspect only top-level assignment keys; never echo malformed source or parse excerpts.
fn provider_declared(text: &str) -> bool {
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            break;
        }
        let Some((key, _)) = line.split_once('=') else {
            continue;
        };
        let Ok(table) = toml::from_str::<toml::Table>(&format!("{key}=true")) else {
            continue;
        };
        if table.contains_key("model_provider") {
            return true;
        }
    }
    false
}

fn parse_config(config: &toml::Table) -> Result<Option<Provider>> {
    let Some(value) = config.get("model_provider") else {
        return Ok(None);
    };
    let id = value
        .as_str()
        .context("Codex model_provider must be a string")?;
    if id == "openai"
        && config
            .get("model_providers")
            .and_then(|providers| providers.get(id))
            .is_none()
    {
        return Ok(None);
    }
    ensure!(
        !id.is_empty()
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "Codex provider id must contain only letters, digits, hyphens or underscores"
    );
    let settings = settings(config, id)?;
    if id == "openai" {
        return Ok(None);
    }
    Ok(Some(Provider {
        id: id.into(),
        settings,
    }))
}

fn settings(config: &toml::Table, id: &str) -> Result<toml::Table> {
    let Some(providers) = config.get("model_providers") else {
        return Ok(toml::Table::new());
    };
    let providers = providers
        .as_table()
        .context("Codex model_providers must be a table")?;
    let Some(settings) = providers.get(id) else {
        return Ok(toml::Table::new());
    };
    let settings = settings
        .as_table()
        .context("Selected Codex provider must be a table")?;
    for (key, value) in settings {
        ensure!(
            !inline_credentials(key, value),
            "Inline Codex provider credentials refused; use env_key / env_http_headers"
        );
        ensure!(
            validation::known_setting(key),
            "Unsupported Codex provider setting: {key}"
        );
        ensure!(
            safe_setting(key, value),
            "Invalid Codex provider setting: {key}; use env_key / env_http_headers for credentials"
        );
    }
    Ok(settings.clone())
}

impl Provider {
    pub(crate) fn apply(&self, cmd: &mut Command) {
        cmd.args([
            "-c",
            &format!("model_provider={}", toml::Value::String(self.id.clone())),
        ]);
        for (key, value) in &self.settings {
            cmd.args(["-c", &format!("model_providers.{}.{key}={value}", self.id)]);
        }
    }
}

pub(crate) fn referenced_env(cmd: &Command) -> Vec<String> {
    if std::path::Path::new(cmd.get_program())
        .file_name()
        .is_none_or(|name| name != "codex")
    {
        return Vec::new();
    }
    let args: Vec<_> = cmd.get_args().collect();
    let mut names = Vec::new();
    for pair in args
        .windows(2)
        .filter(|pair| pair[0] == "-c" || pair[0] == "--config")
    {
        let Some((key, raw)) = pair[1].to_str().and_then(|arg| arg.split_once('=')) else {
            continue;
        };
        if !key.starts_with("model_providers.") {
            continue;
        }
        let Ok(table) = toml::from_str::<toml::Table>(&format!("value={raw}")) else {
            continue;
        };
        if key.ends_with(".env_key") {
            if let Some(name) = table["value"].as_str() {
                names.push(name.to_string());
            }
        } else if key.ends_with(".env_http_headers") {
            if let Some(headers) = table["value"].as_table() {
                names.extend(
                    headers
                        .values()
                        .filter_map(|value| value.as_str().map(String::from)),
                );
            }
        }
    }
    names
}

pub(crate) fn forward_environment(source: &Command, wrapped: &mut Command) -> Vec<String> {
    let names = referenced_env(source);
    for name in &names {
        match source
            .get_envs()
            .find(|(key, _)| *key == std::ffi::OsStr::new(name))
        {
            Some((_, None)) => {
                wrapped.env_remove(name);
            }
            explicit => {
                wrapped.args(["-e", name]);
                if let Some((_, Some(value))) = explicit {
                    wrapped.env(name, value);
                }
            }
        }
    }
    names
}

pub(crate) fn read_only_override(key: &str, value: &serde_json::Value) -> bool {
    if key == "model_provider" {
        return value.is_string();
    }
    let Some((prefix, field)) = key.rsplit_once('.') else {
        return false;
    };
    let Some(id) = prefix.strip_prefix("model_providers.") else {
        return false;
    };
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return false;
    }
    let Ok(value) = toml::Value::try_from(value) else {
        return false;
    };
    safe_setting(field, &value)
}

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "provider_config_tests.rs"]
mod config_tests;
