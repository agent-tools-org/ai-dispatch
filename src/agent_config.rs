// Per-agent default config for persisted model, timeout, and disabled overrides.
// Exports: AgentDefaults, load/save helpers, get_default_model, is_agent_disabled.
// Deps: anyhow, serde, toml, HashMap, crate::paths, crate::types.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentDefaults {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub idle_timeout: Option<u64>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
}

type AgentConfigMap = HashMap<String, AgentDefaults>;

fn config_path() -> PathBuf {
    crate::paths::aid_dir().join("agent_config.toml")
}

fn load_from(path: &Path) -> AgentConfigMap {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| toml::from_str(&content).ok())
        .unwrap_or_default()
}

fn save_to(path: &Path, config: &AgentConfigMap) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, toml::to_string_pretty(config)?)?;
    Ok(())
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn config_key(agent_name: &str) -> String {
    crate::types::AgentKind::parse_str(agent_name)
        .map(|kind| kind.as_str().to_string())
        .unwrap_or_else(|| agent_name.to_string())
}

pub fn load_agent_config() -> HashMap<String, AgentDefaults> {
    load_from(&config_path())
}

pub fn get_default_model(agent_name: &str) -> Option<String> {
    load_agent_config()
        .get(agent_name)
        .and_then(|defaults| defaults.model.clone())
}

pub fn get_default_idle_timeout(agent_name: &str) -> Option<u64> {
    load_agent_config()
        .get(agent_name)
        .and_then(|defaults| defaults.idle_timeout)
}

pub fn is_agent_disabled(agent_name: &str) -> bool {
    load_agent_config()
        .get(&config_key(agent_name))
        .map(|defaults| defaults.disabled)
        .unwrap_or(false)
}

pub fn save_agent_default_model(agent_name: &str, model: Option<&str>) -> Result<()> {
    let path = config_path();
    let mut config = load_from(&path);
    match model {
        Some(model) => {
            config.entry(agent_name.to_string()).or_default().model = Some(model.to_string());
        }
        None => {
            if let Some(defaults) = config.get_mut(agent_name) {
                defaults.model = None;
            }
        }
    }
    config.retain(|_, defaults| !defaults.is_empty());
    save_to(&path, &config)
}

pub fn save_agent_idle_timeout(agent_name: &str, idle_timeout: Option<u64>) -> Result<()> {
    let path = config_path();
    let mut config = load_from(&path);
    config.entry(agent_name.to_string()).or_default().idle_timeout = idle_timeout;
    config.retain(|_, defaults| !defaults.is_empty());
    save_to(&path, &config)
}

pub fn save_agent_disabled(agent_name: &str, disabled: bool) -> Result<()> {
    let path = config_path();
    let mut config = load_from(&path);
    config.entry(config_key(agent_name)).or_default().disabled = disabled;
    config.retain(|_, defaults| !defaults.is_empty());
    save_to(&path, &config)
}

impl AgentDefaults {
    fn is_empty(&self) -> bool {
        self.model.is_none() && self.idle_timeout.is_none() && !self.disabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AidHomeGuard;

    #[test]
    fn save_and_load_default_model_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _guard = AidHomeGuard::set(dir.path());

        save_agent_default_model("cursor", Some("composer-2")).expect("save config");

        let config = load_agent_config();
        assert_eq!(config["cursor"].model.as_deref(), Some("composer-2"));
        assert_eq!(get_default_model("cursor").as_deref(), Some("composer-2"));
    }

    #[test]
    fn clearing_default_model_removes_agent_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _guard = AidHomeGuard::set(dir.path());

        save_agent_default_model("codex", Some("gpt-5.4")).expect("save config");
        save_agent_default_model("codex", None).expect("clear config");

        assert_eq!(get_default_model("codex"), None);
        assert!(load_agent_config().is_empty());
    }

    #[test]
    fn save_and_load_idle_timeout_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _guard = AidHomeGuard::set(dir.path());

        save_agent_idle_timeout("codex", Some(600)).expect("save idle timeout");
        assert_eq!(get_default_idle_timeout("codex"), Some(600));

        save_agent_idle_timeout("codex", None).expect("clear idle timeout");
        assert_eq!(get_default_idle_timeout("codex"), None);
        assert!(load_agent_config().is_empty());
    }

    #[test]
    fn model_and_idle_timeout_coexist() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _guard = AidHomeGuard::set(dir.path());

        save_agent_default_model("cursor", Some("composer-2")).expect("save model");
        save_agent_idle_timeout("cursor", Some(420)).expect("save idle timeout");

        assert_eq!(get_default_model("cursor").as_deref(), Some("composer-2"));
        assert_eq!(get_default_idle_timeout("cursor"), Some(420));

        // Clearing model preserves idle_timeout
        save_agent_default_model("cursor", None).expect("clear model");
        assert_eq!(get_default_model("cursor"), None);
        assert_eq!(get_default_idle_timeout("cursor"), Some(420));
    }

    #[test]
    fn save_and_clear_disabled_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _guard = AidHomeGuard::set(dir.path());

        save_agent_disabled("gemini", true).expect("disable agent");
        assert!(is_agent_disabled("gemini"));
        assert!(load_agent_config()["gemini"].disabled);

        save_agent_disabled("gemini", false).expect("enable agent");
        assert!(!is_agent_disabled("gemini"));
        assert!(load_agent_config().is_empty());
    }

    #[test]
    fn disabled_true_preserves_section_when_model_is_cleared() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _guard = AidHomeGuard::set(dir.path());

        save_agent_default_model("gemini", Some("gemini-3-flash")).expect("save model");
        save_agent_disabled("Gemini", true).expect("disable agent");
        save_agent_default_model("gemini", None).expect("clear model");

        let config = load_agent_config();
        assert_eq!(config["gemini"].model, None);
        assert!(config["gemini"].disabled);
    }
}
