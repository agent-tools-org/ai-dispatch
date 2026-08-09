// Reads aidbar quota snapshots that can override an older route marker.
// Exports: overrides_marker; deps: rate-limit hold classification, aidbar JSON cache.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::types::AgentKind;

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
}

pub(crate) fn overrides_marker(agent: &AgentKind, marker_path: &Path) -> bool {
    let Some(cache_dir) = cache_dir() else {
        return false;
    };
    overrides_marker_in_cache(agent, marker_path, &cache_dir)
}

fn overrides_marker_in_cache(agent: &AgentKind, marker_path: &Path, cache_dir: &Path) -> bool {
    let Some(provider) = provider_name(agent) else {
        return false;
    };
    let Ok(marker_content) = std::fs::read_to_string(marker_path) else {
        return false;
    };
    if !crate::rate_limit::live_quota_can_override(&marker_content, agent) {
        return false;
    }
    let Ok(marker_mtime) = std::fs::metadata(marker_path).and_then(|meta| meta.modified()) else {
        return false;
    };
    let Ok(raw) = std::fs::read(cache_dir.join(format!("{provider}.json"))) else {
        return false;
    };
    let Ok(record) = serde_json::from_slice::<CachedRecord>(&raw) else {
        return false;
    };
    record_overrides(&record, provider, marker_mtime)
}

fn provider_name(agent: &AgentKind) -> Option<&'static str> {
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

fn cache_dir() -> Option<PathBuf> {
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

fn record_overrides(record: &CachedRecord, provider: &str, marker_mtime: SystemTime) -> bool {
    let Some(snapshot) = record.snapshot.as_ref() else {
        return false;
    };
    record.ok
        && snapshot.provider == provider
        && snapshot.fetched_at > DateTime::<Utc>::from(marker_mtime)
        && !snapshot.windows.is_empty()
        && snapshot
            .windows
            .iter()
            .all(|window| (0.0..100.0).contains(&window.used_percent))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(provider: &str, fetched_at: DateTime<Utc>, percentages: &[f64]) -> CachedRecord {
        CachedRecord {
            ok: true,
            snapshot: Some(UsageSnapshot {
                provider: provider.to_string(),
                windows: percentages
                    .iter()
                    .map(|used_percent| UsageWindow { used_percent: *used_percent })
                    .collect(),
                fetched_at,
            }),
        }
    }

    #[test]
    fn newer_snapshot_with_headroom_overrides_marker() {
        let marker_mtime = SystemTime::now();
        let fetched_at = DateTime::<Utc>::from(marker_mtime + std::time::Duration::from_secs(1));
        assert!(record_overrides(
            &record("codex", fetched_at, &[0.0]),
            "codex",
            marker_mtime,
        ));
    }

    #[test]
    fn old_or_equal_snapshot_does_not_override_marker() {
        let marker_mtime = SystemTime::now();
        let fetched_at = DateTime::<Utc>::from(marker_mtime);
        assert!(!record_overrides(
            &record("codex", fetched_at, &[0.0]),
            "codex",
            marker_mtime,
        ));

        let old = DateTime::<Utc>::from(marker_mtime - std::time::Duration::from_secs(1));
        assert!(!record_overrides(
            &record("codex", old, &[0.0]),
            "codex",
            marker_mtime,
        ));
    }

    #[test]
    fn exhausted_or_failed_snapshot_does_not_override_marker() {
        let marker_mtime = SystemTime::now();
        let fetched_at = DateTime::<Utc>::from(marker_mtime + std::time::Duration::from_secs(1));
        assert!(!record_overrides(
            &record("codex", fetched_at, &[100.0]),
            "codex",
            marker_mtime,
        ));

        let mut failed = record("codex", fetched_at, &[0.0]);
        failed.ok = false;
        assert!(!record_overrides(&failed, "codex", marker_mtime));
    }

    #[test]
    fn every_window_needs_headroom_and_snapshot_provider_must_match() {
        let marker_mtime = SystemTime::now();
        let fetched_at = DateTime::<Utc>::from(marker_mtime + std::time::Duration::from_secs(1));
        assert!(!record_overrides(
            &record("codex", fetched_at, &[0.0, 100.0]),
            "codex",
            marker_mtime,
        ));
        assert!(!record_overrides(
            &record("claude", fetched_at, &[0.0]),
            "codex",
            marker_mtime,
        ));
    }

    #[test]
    fn provider_mapping_matches_aidbar_probe_ids() {
        assert_eq!(provider_name(&AgentKind::Codex), Some("codex"));
        assert_eq!(provider_name(&AgentKind::Antigravity), Some("agy"));
        assert_eq!(provider_name(&AgentKind::Copilot), None);
        assert_eq!(provider_name(&AgentKind::Grok), Some("grok"));
        assert_eq!(provider_name(&AgentKind::Qwen), Some("qwen"));
    }

    #[test]
    fn aidbar_error_records_have_no_snapshot_to_override() {
        let raw = r#"{"ok":false,"snapshot":null,"error":"not logged in"}"#;
        let record: CachedRecord = serde_json::from_str(raw).expect("valid aidbar record");
        assert!(!record_overrides(
            &record,
            "grok",
            SystemTime::now() - std::time::Duration::from_secs(1),
        ));
    }

    #[test]
    fn newer_successful_aidbar_cache_record_overrides_marker_from_disk() {
        let temp = tempfile::tempdir().expect("temp directory");
        let marker = temp.path().join("rate-limit-codex");
        let cache_dir = temp.path().join("aidbar");
        std::fs::create_dir_all(&cache_dir).expect("cache directory");
        std::fs::write(&marker, "marker").expect("marker");
        std::fs::write(
            cache_dir.join("codex.json"),
            r#"{"ok":true,"snapshot":{"provider":"codex","plan":"pro","windows":[{"label":"Weekly","used_percent":0.0,"resets_at":null}],"fetched_at":"2099-01-01T00:00:00Z"}}"#,
        )
        .expect("cache record");

        assert!(overrides_marker_in_cache(
            &AgentKind::Codex,
            &marker,
            &cache_dir,
        ));
    }

    #[test]
    fn newer_opencode_headroom_does_not_release_a_needs_human_marker() {
        let temp = tempfile::tempdir().expect("temp directory");
        let marker = temp.path().join("rate-limit-opencode");
        let cache_dir = temp.path().join("aidbar");
        std::fs::create_dir_all(&cache_dir).expect("cache directory");
        std::fs::write(
            cache_dir.join("opencode.json"),
            r#"{"ok":true,"snapshot":{"provider":"opencode","plan":"zen","windows":[{"label":"5h","used_percent":96.86,"resets_at":null},{"label":"Weekly","used_percent":57.08,"resets_at":null}],"fetched_at":"2099-01-01T00:00:00Z"}}"#,
        )
        .expect("cache record");

        for marker_content in [
            "recovery_at: \nhold: manual\nmessage: Insufficient balance\n",
            "recovery_at: \nmessage: Insufficient balance. Manage your billing here\n",
        ] {
            std::fs::write(&marker, marker_content).expect("marker");
            assert!(!overrides_marker_in_cache(
                &AgentKind::OpenCode,
                &marker,
                &cache_dir,
            ));
        }
    }
}
