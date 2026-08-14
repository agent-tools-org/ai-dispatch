// Pre-dispatch model validation and CLI served model probing.
// Exports: validate_model_for_agent, get_served_models_cached, clear_served_models_cache.
// Deps: Agent, AgentKind.

use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};

use crate::types::AgentKind;
use super::Agent;

static SERVED_CACHE: OnceLock<Mutex<HashMap<AgentKind, Option<Vec<String>>>>> = OnceLock::new();

#[cfg(test)]
thread_local! {
    static TEST_OVERRIDE: std::cell::RefCell<HashMap<AgentKind, Option<Vec<String>>>> =
        std::cell::RefCell::new(HashMap::new());
}

pub(crate) const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const SERVED_MODELS_CACHE_TTL: Duration = Duration::from_secs(24 * 3600);

fn cache() -> &'static Mutex<HashMap<AgentKind, Option<Vec<String>>>> {
    SERVED_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_file_path() -> std::path::PathBuf {
    crate::paths::aid_dir().join("served_models_cache.json")
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct ServedModelsCacheEntry {
    pub models: Vec<String>,
    pub updated_at_secs: u64,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum DiskCacheItem {
    New(ServedModelsCacheEntry),
    Legacy(Vec<String>),
}

fn load_from_disk_cache(kind: AgentKind) -> Option<Vec<String>> {
    let path = cache_file_path();
    let content = std::fs::read_to_string(&path).ok()?;
    let map: HashMap<String, DiskCacheItem> = serde_json::from_str(&content).ok()?;
    match map.get(kind.as_str())? {
        DiskCacheItem::New(entry) => {
            let age = now_secs().saturating_sub(entry.updated_at_secs);
            if age <= SERVED_MODELS_CACHE_TTL.as_secs() {
                Some(entry.models.clone())
            } else {
                None
            }
        }
        DiskCacheItem::Legacy(_) => None,
    }
}

fn save_to_disk_cache(kind: AgentKind, models: &[String]) {
    let path = cache_file_path();
    let mut map: HashMap<String, ServedModelsCacheEntry> = if let Ok(content) = std::fs::read_to_string(&path) {
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        HashMap::new()
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    map.insert(
        kind.as_str().to_string(),
        ServedModelsCacheEntry {
            models: models.to_vec(),
            updated_at_secs: now_secs(),
        },
    );
    if let Ok(json) = serde_json::to_string_pretty(&map) {
        let _ = std::fs::write(&path, json);
    }
}

pub(crate) fn clear_served_models_cache() {
    if let Ok(mut guard) = cache().lock() {
        guard.clear();
    }
    let _ = std::fs::remove_file(cache_file_path());
}

pub(crate) fn clear_served_models_cache_for_agent(kind: AgentKind) {
    if let Ok(mut guard) = cache().lock() {
        guard.remove(&kind);
    }
    let path = cache_file_path();
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(mut map) = serde_json::from_str::<HashMap<String, serde_json::Value>>(&content) {
            if map.remove(kind.as_str()).is_some() {
                if let Ok(json) = serde_json::to_string_pretty(&map) {
                    let _ = std::fs::write(&path, json);
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) struct MockServedModelsGuard;

#[cfg(test)]
impl MockServedModelsGuard {
    pub fn set(kind: AgentKind, models: Option<Vec<String>>) -> Self {
        TEST_OVERRIDE.with(|cell| {
            cell.borrow_mut().insert(kind, models);
        });
        Self
    }
}

#[cfg(test)]
impl Drop for MockServedModelsGuard {
    fn drop(&mut self) {
        TEST_OVERRIDE.with(|cell| {
            cell.borrow_mut().clear();
        });
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum ModelSource {
    UserSupplied,
    AidResolved,
}

impl Default for ModelSource {
    /// Unknown provenance belongs to old persisted rows; fail closed so a
    /// model whose caller intent cannot be recovered is never silently dropped.
    fn default() -> Self {
        Self::UserSupplied
    }
}

pub(crate) fn validate_model_for_agent(
    agent: &dyn Agent,
    model: &str,
    source: ModelSource,
) -> Result<bool> {
    let model_clean = model.trim();
    if model_clean.is_empty() {
        return Ok(true);
    }
    let kind = agent.kind();
    let served = get_served_models_cached(agent);

    let Some(served_list) = served else {
        aid_info!(
            "[aid] Cannot query served models for {}; allowing dispatch with model '{model_clean}'",
            kind.as_str()
        );
        return Ok(true);
    };

    if served_list.is_empty() {
        aid_info!(
            "[aid] No served models reported for {}; allowing dispatch with model '{model_clean}'",
            kind.as_str()
        );
        return Ok(true);
    }

    if served_list.iter().any(|m| m.eq_ignore_ascii_case(model_clean)) {
        return Ok(true);
    }

    // Model not in cached list; refresh cache once in case CLI gained support for new models.
    let fresh_served_list = refresh_served_models_cached(agent).unwrap_or(served_list);
    if fresh_served_list.iter().any(|m| m.eq_ignore_ascii_case(model_clean)) {
        return Ok(true);
    }

    let list_str = fresh_served_list.join(", ");
    if source == ModelSource::UserSupplied {
        return Err(anyhow!(
            "Agent '{}' does not serve model '{model_clean}'. Served models: {list_str}",
            kind.as_str()
        ));
    }
    aid_warn!(
        "[aid] Agent '{}' does not serve aid-selected model '{model_clean}'; dropping it and using the agent's own default model",
        kind.as_str()
    );
    Ok(false)
}

pub(crate) fn refresh_served_models_cached(agent: &dyn Agent) -> Option<Vec<String>> {
    let kind = agent.kind();
    let result = agent.served_models().ok().flatten();
    if let Ok(mut guard) = cache().lock() {
        guard.insert(kind, result.clone());
    }
    if let Some(ref models) = result {
        save_to_disk_cache(kind, models);
    } else {
        clear_served_models_cache_for_agent(kind);
    }
    result
}

pub(crate) fn get_served_models_cached(agent: &dyn Agent) -> Option<Vec<String>> {
    let kind = agent.kind();
    #[cfg(test)]
    {
        let thread_mock = TEST_OVERRIDE.with(|cell| cell.borrow().get(&kind).cloned());
        if let Some(res) = thread_mock {
            return res;
        }
    }

    if let Ok(guard) = cache().lock() {
        if let Some(cached) = guard.get(&kind) {
            return cached.clone();
        }
    }

    if let Some(disk_models) = load_from_disk_cache(kind) {
        if let Ok(mut guard) = cache().lock() {
            guard.insert(kind, Some(disk_models.clone()));
        }
        return Some(disk_models);
    }

    refresh_served_models_cached(agent)
}

pub(crate) fn run_probe_cmd(cmd: Command) -> Option<String> {
    run_cmd_with_timeout(cmd, DEFAULT_PROBE_TIMEOUT)
}

pub(crate) fn run_cmd_with_timeout(mut cmd: Command, timeout: Duration) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let res = cmd.output().ok().and_then(|output| {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                Some(format!("{stdout}\n{stderr}"))
            } else {
                None
            }
        });
        let _ = tx.send(res);
    });
    rx.recv_timeout(timeout).ok().flatten()
}

#[cfg(test)]
#[path = "model_validation_tests.rs"]
mod tests;
