// Credential pool: multiple API keys per provider with rotation strategies.
// Exports: CredentialPool, load_pool(), get_credential(), mark_exhausted().
// Deps: serde/toml, std::fs, chrono, crate::paths.

use chrono::{DateTime, Duration, Local};
use serde::Deserialize;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct CredentialPool {
    #[serde(flatten)]
    pub providers: HashMap<String, ProviderPool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderPool {
    pub strategy: Strategy,
    pub keys: Vec<KeyEntry>,
    #[serde(skip)]
    next_index: Cell<usize>,
    #[serde(skip)]
    usage_counts: RefCell<Vec<u64>>,
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
    load_pool_from_path(&crate::paths::aid_dir().join("credentials.toml"))
}

pub fn get_credential(pool: &CredentialPool, agent: &str) -> Option<(String, String)> {
    let provider = pool.providers.get(agent)?;
    let now = Local::now();
    let mut usage_counts = provider.usage_counts.borrow_mut();
    if usage_counts.len() != provider.keys.len() {
        usage_counts.resize(provider.keys.len(), 0);
    }
    let selection = match provider.strategy {
        Strategy::FillFirst => select_fill_first(&provider.keys, &now),
        Strategy::RoundRobin => select_round_robin(provider, &now),
        Strategy::LeastUsed => select_least_used(&provider.keys, &usage_counts, &now),
    }?;
    usage_counts[selection.0] = usage_counts[selection.0].saturating_add(1);
    Some((provider.keys[selection.0].env.clone(), selection.1))
}

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

fn select_fill_first(keys: &[KeyEntry], now: &DateTime<Local>) -> Option<(usize, String)> {
    keys.iter()
        .enumerate()
        .find_map(|(index, key)| key_value_if_ready(key, now).map(|value| (index, value)))
}

fn select_round_robin(provider: &ProviderPool, now: &DateTime<Local>) -> Option<(usize, String)> {
    let len = provider.keys.len();
    if len == 0 {
        return None;
    }
    let start = provider.next_index.get() % len;
    for offset in 0..len {
        let index = (start + offset) % len;
        if let Some(value) = key_value_if_ready(&provider.keys[index], now) {
            provider.next_index.set((index + 1) % len);
            return Some((index, value));
        }
    }
    None
}

fn select_least_used(
    keys: &[KeyEntry],
    usage_counts: &[u64],
    now: &DateTime<Local>,
) -> Option<(usize, String)> {
    let mut best: Option<(usize, u64, String)> = None;
    for (index, key) in keys.iter().enumerate() {
        let Some(value) = key_value_if_ready(key, now) else {
            continue;
        };
        let usage = usage_counts.get(index).copied().unwrap_or(0);
        if best.as_ref().is_none_or(|(_, best_usage, _)| usage < *best_usage) {
            best = Some((index, usage, value));
        }
    }
    best.map(|(index, _, value)| (index, value))
}

fn key_value_if_ready(key: &KeyEntry, now: &DateTime<Local>) -> Option<String> {
    if key.exhausted_until.as_ref().is_some_and(|until| *until > *now) {
        return None;
    }
    std::env::var(&key.env).ok().filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{CredentialPool, Strategy, get_credential, load_pool, load_pool_from_path, mark_exhausted};
    use crate::paths::AidHomeGuard;
    use std::fs;
    use tempfile::TempDir;

    const SAMPLE: &str = r#"
[codex]
strategy = "round_robin"
keys = [
  { name = "personal", env = "OPENAI_API_KEY" },
  { name = "team", env = "OPENAI_API_KEY_2" },
]

[gemini]
strategy = "fill_first"
keys = [
  { name = "default", env = "GEMINI_API_KEY" },
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
    fn round_robin_returns_keys_in_order() {
        let dir = TempDir::new().unwrap();
        write_pool(&dir);
        let pool: CredentialPool = load_pool_from_path(&dir.path().join("credentials.toml")).unwrap();
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "first");
            std::env::set_var("OPENAI_API_KEY_2", "second");
        }

        let first = get_credential(&pool, "codex");
        let second = get_credential(&pool, "codex");
        let third = get_credential(&pool, "codex");

        assert_eq!(first, Some(("OPENAI_API_KEY".to_string(), "first".to_string())));
        assert_eq!(second, Some(("OPENAI_API_KEY_2".to_string(), "second".to_string())));
        assert_eq!(third, Some(("OPENAI_API_KEY".to_string(), "first".to_string())));
    }

    #[test]
    fn fill_first_returns_first_non_exhausted_key() {
        let dir = TempDir::new().unwrap();
        write_pool(&dir);
        let mut pool = load_pool_from_path(&dir.path().join("credentials.toml")).unwrap();
        pool.providers.get_mut("gemini").unwrap().keys.push(super::KeyEntry {
            name: "backup".to_string(),
            env: "GEMINI_API_KEY_BACKUP".to_string(),
            exhausted_until: None,
        });
        unsafe {
            std::env::set_var("GEMINI_API_KEY", "primary");
            std::env::set_var("GEMINI_API_KEY_BACKUP", "backup");
        }

        mark_exhausted(&mut pool, "gemini", "default", 5);

        let selected = get_credential(&pool, "gemini");

        assert_eq!(
            selected,
            Some(("GEMINI_API_KEY_BACKUP".to_string(), "backup".to_string()))
        );
    }

    #[test]
    fn mark_exhausted_skips_exhausted_keys() {
        let dir = TempDir::new().unwrap();
        write_pool(&dir);
        let mut pool = load_pool_from_path(&dir.path().join("credentials.toml")).unwrap();
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "first");
            std::env::set_var("OPENAI_API_KEY_2", "second");
        }

        mark_exhausted(&mut pool, "codex", "personal", 5);

        let selected = get_credential(&pool, "codex");

        assert_eq!(selected, Some(("OPENAI_API_KEY_2".to_string(), "second".to_string())));
    }

    #[test]
    fn missing_file_returns_none() {
        let dir = TempDir::new().unwrap();
        let _guard = AidHomeGuard::set(dir.path());

        assert!(load_pool().is_none());
    }
}
