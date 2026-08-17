// Reads aidbar quota snapshots. Parse only; override policy lives in route_availability.
// Exports: snapshot, overrides_marker, provider_name, cache_dir.
// Deps: serde, chrono, route_availability types.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::route_availability::{ProbeEvidence, WindowView};
use crate::types::AgentKind;

const STALE_AFTER: Duration = Duration::from_secs(15 * 60);

#[derive(Deserialize)]
struct CachedRecord {
    ok: bool,
    snapshot: Option<UsageSnapshot>,
}

#[derive(Deserialize)]
struct UsageSnapshot {
    provider: String,
    windows: Vec<UsageWindow>,
    fetched_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct UsageWindow {
    used_percent: f64,
    #[serde(default)]
    label: String,
    #[serde(default)]
    resets_at: Option<DateTime<Utc>>,
    #[serde(default)]
    group: Option<String>,
}

pub(crate) fn overrides_marker(agent: &AgentKind, marker_path: &Path) -> bool {
    crate::route_availability::overrides_marker_at(agent, marker_path)
}

/// Parse the aidbar cache for this agent. No override policy.
pub(crate) fn snapshot(agent: &AgentKind) -> Option<ProbeEvidence> {
    snapshot_in_cache(agent, &cache_dir()?)
}

pub(crate) fn snapshot_in_cache(agent: &AgentKind, cache_dir: &Path) -> Option<ProbeEvidence> {
    let provider = provider_name(agent)?;
    let raw = std::fs::read(cache_dir.join(format!("{provider}.json"))).ok()?;
    let record: CachedRecord = serde_json::from_slice(&raw).ok()?;
    probe_from_record(&record, provider)
}

fn probe_from_record(record: &CachedRecord, expected_provider: &str) -> Option<ProbeEvidence> {
    let snap = record.snapshot.as_ref()?;
    if snap.provider != expected_provider {
        return None;
    }
    let age = Utc::now()
        .signed_duration_since(snap.fetched_at)
        .to_std()
        .unwrap_or(Duration::ZERO);
    Some(ProbeEvidence {
        provider: snap.provider.clone(),
        fetched_at: snap.fetched_at,
        age,
        stale: age >= STALE_AFTER,
        ok: record.ok,
        windows: snap
            .windows
            .iter()
            .map(|window| WindowView {
                label: window.label.clone(),
                used_percent: window.used_percent,
                resets_at: window.resets_at,
                group: window.group.clone(),
            })
            .collect(),
    })
}

pub(crate) fn provider_name(agent: &AgentKind) -> Option<&'static str> {
    match agent {
        AgentKind::Codex => Some("codex"),
        AgentKind::Claude => Some("claude"),
        AgentKind::OpenCode => Some("opencode"),
        AgentKind::Cursor => Some("cursor"),
        AgentKind::Antigravity => Some("agy"),
        AgentKind::Grok => Some("grok"),
        AgentKind::Qwen => Some("qwen"),
        _ => None,
    }
}

pub(crate) fn cache_dir() -> Option<PathBuf> {
    #[cfg(test)]
    {
        // Hold readers now consult the snapshot. A test that only isolates
        // AID_HOME must not inherit the host aidbar cache and silently release.
        return CACHE_DIR_OVERRIDE.with(|cell| cell.borrow().clone());
    }
    #[cfg(not(test))]
    {
        std::env::var_os("XDG_CACHE_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .map(|home| PathBuf::from(home).join(".cache"))
            })
            .map(|dir| dir.join("aidbar"))
    }
}

#[cfg(test)]
thread_local! {
    static CACHE_DIR_OVERRIDE: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) struct CacheDirGuard {
    previous: Option<PathBuf>,
}

#[cfg(test)]
impl CacheDirGuard {
    pub(crate) fn set(path: &Path) -> Self {
        let previous = CACHE_DIR_OVERRIDE.with(|cell| cell.borrow().clone());
        CACHE_DIR_OVERRIDE.with(|cell| *cell.borrow_mut() = Some(path.to_path_buf()));
        Self { previous }
    }
}

#[cfg(test)]
impl Drop for CacheDirGuard {
    fn drop(&mut self) {
        CACHE_DIR_OVERRIDE.with(|cell| *cell.borrow_mut() = self.previous.take());
    }
}

#[cfg(test)]
#[path = "live_quota_tests.rs"]
mod tests;
