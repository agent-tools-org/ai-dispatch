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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct ServedModelsCacheEntry {
    pub models: Vec<String>,
    pub updated_at_secs: u64,
}

#[derive(Clone, Debug)]
struct CachedServedModels {
    models: Option<Vec<String>>,
    from_live_probe: bool,
}

static SERVED_CACHE: OnceLock<Mutex<HashMap<AgentKind, CachedServedModels>>> = OnceLock::new();

#[cfg(test)]
thread_local! {
    static TEST_OVERRIDE: std::cell::RefCell<HashMap<AgentKind, Option<Vec<String>>>> =
        std::cell::RefCell::new(HashMap::new());
}

pub(crate) const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const SERVED_MODELS_CACHE_TTL: Duration = Duration::from_secs(24 * 3600);

fn cache() -> &'static Mutex<HashMap<AgentKind, CachedServedModels>> {
    SERVED_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_file_path() -> std::path::PathBuf {
    crate::paths::aid_dir().join("served_models_cache.json")
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn load_from_disk_cache(kind: AgentKind) -> Option<Vec<String>> {
    let path = cache_file_path();
    let content = std::fs::read_to_string(&path).ok()?;
    let map: HashMap<String, ServedModelsCacheEntry> = serde_json::from_str(&content).ok()?;
    let entry = map.get(kind.as_str())?;
    let age = now_secs().saturating_sub(entry.updated_at_secs);
    if age <= SERVED_MODELS_CACHE_TTL.as_secs() {
        Some(entry.models.clone())
    } else {
        None
    }
}

fn atomic_write_cache_file(path: &std::path::Path, content: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    static WRITE_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let count = WRITE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp_path = path.with_extension(format!("tmp.{}.{}.{}", std::process::id(), nanos, count));
    if std::fs::write(&tmp_path, content).is_ok() && std::fs::rename(&tmp_path, path).is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
}

fn save_to_disk_cache(kind: AgentKind, models: &[String]) {
    let path = cache_file_path();
    let mut map: HashMap<String, ServedModelsCacheEntry> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default();
    map.insert(
        kind.as_str().to_string(),
        ServedModelsCacheEntry {
            models: models.to_vec(),
            updated_at_secs: now_secs(),
        },
    );
    if let Ok(json) = serde_json::to_string_pretty(&map) {
        atomic_write_cache_file(&path, &json);
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
                    atomic_write_cache_file(&path, &json);
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
    
    // Use fast path to never slow dispatch
    let served = get_served_models_fast(agent);

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

    let list_str = served_list.join(", ");
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
        guard.insert(
            kind,
            CachedServedModels {
                models: result.clone(),
                from_live_probe: true,
            },
        );
    }
    if let Some(ref models) = result {
        save_to_disk_cache(kind, models);
    } else {
        clear_served_models_cache_for_agent(kind);
    }
    result
}

pub(crate) fn get_served_models_cached(agent: &dyn Agent) -> Option<Vec<String>> {
    get_served_models_cached_with_status(agent).0
}

pub(crate) fn get_served_models_fast(agent: &dyn Agent) -> Option<Vec<String>> {
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
            return cached.models.clone();
        }
    }

    if let Some(disk_models) = load_from_disk_cache(kind) {
        if let Ok(mut guard) = cache().lock() {
            guard.insert(
                kind,
                CachedServedModels {
                    models: Some(disk_models.clone()),
                    from_live_probe: false,
                },
            );
        }
        return Some(disk_models);
    }

    // Fire off a background probe so it's ready next time
    std::thread::spawn(move || {
        let bg_agent = crate::agent::get_agent(kind);
        refresh_served_models_cached(&*bg_agent);
    });

    None
}

pub(crate) fn get_served_models_cached_with_status(agent: &dyn Agent) -> (Option<Vec<String>>, bool) {
    let kind = agent.kind();
    #[cfg(test)]
    {
        let thread_mock = TEST_OVERRIDE.with(|cell| cell.borrow().get(&kind).cloned());
        if let Some(res) = thread_mock {
            return (res, true);
        }
    }

    if let Ok(guard) = cache().lock() {
        if let Some(cached) = guard.get(&kind) {
            return (cached.models.clone(), cached.from_live_probe);
        }
    }

    if let Some(disk_models) = load_from_disk_cache(kind) {
        if let Ok(mut guard) = cache().lock() {
            guard.insert(
                kind,
                CachedServedModels {
                    models: Some(disk_models.clone()),
                    from_live_probe: false,
                },
            );
        }
        return (Some(disk_models), false);
    }

    let fresh = refresh_served_models_cached(agent);
    (fresh, true)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProbeOutput {
    pub stdout: String,
    pub stderr: String,
}

pub(crate) fn run_probe_cmd(cmd: Command) -> Option<ProbeOutput> {
    run_cmd_with_timeout(cmd, DEFAULT_PROBE_TIMEOUT)
}

pub(crate) fn run_cmd_with_timeout(mut cmd: Command, timeout: Duration) -> Option<ProbeOutput> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let res = cmd.output().ok().and_then(|output| {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                Some(ProbeOutput { stdout, stderr })
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
