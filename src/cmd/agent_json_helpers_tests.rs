// Probe-backed quota JSON: Degraded vs Ok from a live aidbar snapshot.
// Deps: build_quota_json, isolated AID_HOME, CacheDirGuard.

use super::build_quota_json;
use crate::live_quota::CacheDirGuard;
use crate::paths::{self, AidHomeGuard};
use crate::types::AgentKind;
use chrono::Utc;

fn isolated() -> (tempfile::TempDir, AidHomeGuard, CacheDirGuard) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let _ = std::fs::create_dir_all(tmp.path().join(".aid"));
    let home = AidHomeGuard::set(tmp.path());
    std::fs::create_dir_all(paths::aid_dir()).ok();
    let aidbar = tmp.path().join("aidbar");
    std::fs::create_dir_all(&aidbar).ok();
    let cache = CacheDirGuard::set(&aidbar);
    (tmp, home, cache)
}

fn write_codex_probe(cache: &std::path::Path, used: f64) {
    let fetched = Utc::now().to_rfc3339();
    std::fs::write(
        cache.join("codex.json"),
        format!(
            r#"{{"ok":true,"snapshot":{{"provider":"codex","windows":[{{"label":"Weekly","used_percent":{used},"resets_at":"2026-09-19T08:10:00Z"}}],"fetched_at":"{fetched}"}}}}"#
        ),
    )
    .expect("cache");
}

#[test]
fn quota_json_degraded_at_96_percent_from_probe() {
    let (tmp, _home, _cache) = isolated();
    write_codex_probe(&tmp.path().join("aidbar"), 96.0);
    let q = build_quota_json(&AgentKind::Codex, None);
    assert_eq!(q.state, "degraded");
    assert_eq!(q.used_percent, Some(96.0));
    assert_eq!(q.resets_at.as_deref(), Some("2026-09-19T08:10:00+00:00"));
    assert_eq!(q.window.as_deref(), Some("Weekly"));
    assert_eq!(q.source, "probe");
    assert!(!q.stale);
    assert!(q.groups.is_empty());
}

#[test]
fn quota_json_ok_at_20_percent_from_probe() {
    let (tmp, _home, _cache) = isolated();
    write_codex_probe(&tmp.path().join("aidbar"), 20.0);
    let q = build_quota_json(&AgentKind::Codex, None);
    assert_eq!(q.state, "ok");
    assert_eq!(q.used_percent, Some(20.0));
    assert_eq!(q.resets_at.as_deref(), Some("2026-09-19T08:10:00+00:00"));
    assert_eq!(q.window.as_deref(), Some("Weekly"));
    assert_eq!(q.source, "probe");
    assert!(!q.stale);
}
