// Pre-dispatch model validation and CLI served model probing.
// Exports: validate_model_for_agent, get_served_models_cached, clear_served_models_cache.
// Deps: Agent, AgentKind.

use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};

use crate::types::AgentKind;
use super::Agent;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct ServedModelsCacheEntry {
    pub models: Vec<String>,
    pub updated_at_secs: u64,
    #[serde(default)]
    pub fingerprint: Option<String>,
}

#[derive(Clone, Debug)]
struct CachedServedModels {
    models: Option<Vec<String>>,
    from_live_probe: bool,
    fingerprint: Option<String>,
}

#[cfg(not(test))]
static SERVED_CACHE: std::sync::OnceLock<Mutex<HashMap<AgentKind, CachedServedModels>>> = std::sync::OnceLock::new();

pub(crate) const SERVED_MODELS_CACHE_TTL: Duration = Duration::from_secs(24 * 3600);

fn cache() -> &'static Mutex<HashMap<AgentKind, CachedServedModels>> {
    #[cfg(test)]
    return cache_tests::cache();
    #[cfg(not(test))]
    SERVED_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_file_path() -> std::path::PathBuf {
    crate::paths::aid_dir().join("served_models_cache.json")
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

pub(crate) fn load_from_disk_cache(kind: AgentKind) -> Option<Vec<String>> {
    if kind == AgentKind::Custom {
        return load_disk_entry(kind, None).map(|entry| entry.models);
    }
    let agent = super::get_agent(kind);
    let fingerprint = agent.served_models_fingerprint();
    if fingerprint.is_some() {
        return get_served_models_cached(agent.as_ref());
    }
    load_disk_entry(kind, None).map(|entry| entry.models)
}

fn load_disk_entry(kind: AgentKind, fingerprint: Option<&str>) -> Option<ServedModelsCacheEntry> {
    let content = std::fs::read_to_string(cache_file_path()).ok()?;
    let mut map: HashMap<String, ServedModelsCacheEntry> = serde_json::from_str(&content).ok()?;
    let entry = map.remove(kind.as_str())?;
    let age = now_secs().saturating_sub(entry.updated_at_secs);
    if age <= SERVED_MODELS_CACHE_TTL.as_secs()
        && fingerprint.is_none_or(|fp| entry.fingerprint.as_deref() == Some(fp)) {
        Some(entry)
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

fn save_to_disk_cache(kind: AgentKind, models: &[String], fingerprint: Option<String>) {
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
            fingerprint,
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
    let (served, from_live_probe) = get_served_models_cached_with_status(agent);

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

    // Model not in cached list; refresh cache once ONLY if cached list did NOT come from a live probe in this process.
    let fresh_served_list = if from_live_probe {
        served_list
    } else {
        refresh_served_models_cached(agent).unwrap_or(served_list)
    };

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
    let fingerprint = agent.served_models_fingerprint();
    let result = agent.served_models().ok().flatten();
    if let Ok(mut guard) = cache().lock() {
        guard.insert(
            kind,
            CachedServedModels {
                models: result.clone(),
                from_live_probe: true,
                fingerprint: fingerprint.clone(),
            },
        );
    }
    if let Some(ref models) = result {
        save_to_disk_cache(kind, models, fingerprint);
    } else {
        clear_served_models_cache_for_agent(kind);
    }
    result
}

pub(crate) fn get_served_models_cached(agent: &dyn Agent) -> Option<Vec<String>> {
    get_served_models_cached_with_status(agent).0
}

pub(crate) fn get_served_models_cached_with_status(agent: &dyn Agent) -> (Option<Vec<String>>, bool) {
    let kind = agent.kind();
    #[cfg(test)]
    if let Some(res) = cache_tests::mock_models(kind) { return (res, true); }

    let fingerprint = agent.served_models_fingerprint();
    if let Ok(guard) = cache().lock() {
        if let Some(cached) = guard.get(&kind).filter(|entry|
            fingerprint.as_ref().is_none_or(|fp| entry.fingerprint.as_ref() == Some(fp))) {
            return (cached.models.clone(), cached.from_live_probe);
        }
    }

    if let Some(entry) = load_disk_entry(kind, fingerprint.as_deref()) {
        let disk_models = entry.models;
        if let Ok(mut guard) = cache().lock() {
            guard.insert(
                kind,
                CachedServedModels {
                    models: Some(disk_models.clone()),
                    from_live_probe: false,
                    fingerprint: entry.fingerprint,
                },
            );
        }
        return (Some(disk_models), false);
    }

    let fresh = refresh_served_models_cached(agent);
    (fresh, true)
}

pub(crate) fn run_probe_cmd(program: &str, args: &[&str]) -> Option<super::env::CliCommandOutput> {
    super::env_identity::run_bounded(program, args, super::env_identity::DEFAULT_PROBE_TIMEOUT)
        .ok().filter(|output| output.success)
}

#[cfg(test)]
#[path = "model_validation_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "model_validation_cache_tests.rs"]
mod cache_tests;
#[cfg(test)]
pub(crate) use cache_tests::MockServedModelsGuard;
