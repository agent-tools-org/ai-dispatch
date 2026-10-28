// Credential pool: multiple API keys per provider with rotation strategies.
// Exports: CredentialPool, load_pool().
// Deps: serde/toml, std::fs, chrono, crate::support::paths.

use chrono::{DateTime, Local};
#[cfg(test)]
use chrono::Duration;
use serde::Deserialize;
#[cfg(test)]
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

#[cfg(test)]
thread_local! {
    static TEST_ENV: RefCell<Option<HashMap<String, String>>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_test_env(values: Option<HashMap<String, String>>) {
    TEST_ENV.with(|cell| *cell.borrow_mut() = values);
}

#[derive(Debug, Clone, Deserialize)]
pub struct CredentialPool {
    #[serde(flatten)]
    pub providers: HashMap<String, ProviderPool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderPool {
    pub strategy: Strategy,
    pub keys: Vec<KeyEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KeyEntry {
    pub name: String,
    pub env: String,
    #[serde(skip)]
    pub exhausted_until: Option<DateTime<Local>>,
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    #[default]
    FillFirst,
    RoundRobin,
    LeastUsed,
}

pub fn load_pool() -> Option<CredentialPool> {
    load_pool_from_path(&crate::support::paths::aid_dir().join("credentials.toml"))
}

#[cfg(test)]
pub fn mark_exhausted(
    pool: &mut CredentialPool,
    agent: &str,
    key_name: &str,
    cooldown_mins: u64,
) {
    let Some(provider) = pool.providers.get_mut(agent) else {
        return;
    };
    let cooldown = match i64::try_from(cooldown_mins) {
        Ok(value) => value,
        Err(_) => i64::MAX,
    };
    let exhausted_until = Local::now() + Duration::minutes(cooldown);
    if let Some(key) = provider.keys.iter_mut().find(|key| key.name == key_name) {
        key.exhausted_until = Some(exhausted_until);
    }
}

fn load_pool_from_path(path: &Path) -> Option<CredentialPool> {
    let contents = std::fs::read_to_string(path).ok()?;
    toml::from_str(&contents).ok()
}

fn credential_env(name: &str) -> Option<String> {
    #[cfg(test)]
    if let Some(values) = TEST_ENV.with(|cell| cell.borrow().clone()) {
        return values.get(name).cloned();
    }
    std::env::var(name).ok()
}

pub(crate) fn credential_is_set(name: &str) -> bool {
    credential_env(name).is_some_and(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{Strategy, load_pool, load_pool_from_path};

    use std::fs;
    use tempfile::TempDir;

    const SAMPLE: &str = r#"
[codex]
strategy = "round_robin"
keys = [
  { name = "personal", env = "OPENAI_API_KEY" },
  { name = "team", env = "OPENAI_API_KEY_2" },
]

[grok]
strategy = "fill_first"
keys = [
  { name = "default", env = "XAI_API_KEY" },
]
"#;

    fn write_pool(dir: &TempDir) {
        fs::write(dir.path().join("credentials.toml"), SAMPLE).unwrap();
    }

    #[test]
    fn load_pool_parses_sample_toml() {
        let dir = TempDir::new().unwrap();
        write_pool(&dir);

        let pool = load_pool_from_path(&dir.path().join("credentials.toml")).unwrap();

        assert_eq!(pool.providers.len(), 2);
        assert_eq!(pool.providers["codex"].strategy, Strategy::RoundRobin);
        assert_eq!(pool.providers["codex"].keys[0].name, "personal");
    }

    #[test]
    fn missing_file_returns_none() {
        let _dir = crate::support::test_env::TestHome::new();

        assert!(load_pool().is_none());
    }
}
