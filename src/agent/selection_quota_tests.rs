// Scoring-only quota regression cases using the aidbar cache record shape.

use chrono::{Duration, SecondsFormat, Utc};
use serde_json::{Value, json};
use tempfile::TempDir;

use super::headroom_penalty;
use crate::live_quota::CacheDirGuard;
use crate::paths::AidHomeGuard;
use crate::types::AgentKind;

fn isolated() -> (TempDir, AidHomeGuard, CacheDirGuard) {
    let temp = TempDir::new().expect("temp dir");
    let home = AidHomeGuard::set(temp.path());
    let cache = temp.path().join("aidbar");
    std::fs::create_dir_all(&cache).expect("cache dir");
    let guard = CacheDirGuard::set(&cache);
    (temp, home, guard)
}

fn write_snapshot(temp: &TempDir, provider: &str, windows: Vec<Value>, ok: bool, age_mins: i64) {
    let fetched_at = (Utc::now() - Duration::minutes(age_mins))
        .to_rfc3339_opts(SecondsFormat::Secs, true);
    let record = json!({
        "ok": ok,
        "snapshot": {"provider": provider, "windows": windows, "fetched_at": fetched_at}
    });
    std::fs::write(
        temp.path().join("aidbar").join(format!("{provider}.json")),
        record.to_string(),
    )
    .expect("snapshot");
}

#[test]
fn selection_headroom_only_uses_requested_model_group() {
    let (temp, _home, _cache) = isolated();
    let reset = (Utc::now() + Duration::hours(5)).to_rfc3339();
    write_snapshot(&temp, "agy", vec![
        json!({"label": "Claude and GPT models 5h", "used_percent": 5.0, "resets_at": reset, "group": "claude-gpt"}),
        json!({"label": "Gemini Models Weekly", "used_percent": 97.0, "resets_at": reset, "group": "gemini"}),
    ], true, 1);

    assert_eq!(headroom_penalty(AgentKind::Antigravity, Some("claude-sonnet-4-6")), 0.0);
    assert_eq!(headroom_penalty(AgentKind::Antigravity, Some("gpt-oss-120b-medium")), 0.0);
    assert_eq!(headroom_penalty(AgentKind::Antigravity, Some("gemini-3.6-flash-low")), -6.0);
    assert_eq!(headroom_penalty(AgentKind::Antigravity, None), -6.0);
}

#[test]
fn selection_headroom_missing_group_window_does_not_borrow_other_pool() {
    let (temp, _home, _cache) = isolated();
    write_snapshot(&temp, "agy", vec![
        json!({"label": "Gemini Models Weekly", "used_percent": 97.0, "group": "gemini"}),
    ], true, 1);
    assert_eq!(headroom_penalty(AgentKind::Antigravity, Some("claude-sonnet-4-6")), 0.0);
}

#[test]
fn selection_headroom_ignores_short_window_resetting_soon() {
    let (temp, _home, _cache) = isolated();
    let soon = (Utc::now() + Duration::minutes(20)).to_rfc3339();
    write_snapshot(&temp, "codex", vec![
        json!({"label": "5h", "used_percent": 85.0, "resets_at": soon}),
        json!({"label": "Weekly", "used_percent": 30.0}),
    ], true, 1);
    assert_eq!(headroom_penalty(AgentKind::Codex, Some("gpt-5.5")), 0.0);
}

#[test]
fn selection_headroom_short_window_counts_when_distant_or_nearly_exhausted() {
    let (temp, _home, _cache) = isolated();
    let distant = (Utc::now() + Duration::minutes(61)).to_rfc3339();
    let soon = (Utc::now() + Duration::minutes(20)).to_rfc3339();
    write_snapshot(&temp, "codex", vec![
        json!({"label": "5h", "used_percent": 85.0, "resets_at": distant}),
        json!({"label": "Weekly", "used_percent": 30.0}),
    ], true, 1);
    assert_eq!(headroom_penalty(AgentKind::Codex, None), -3.0);

    write_snapshot(&temp, "codex", vec![
        json!({"label": "5h", "used_percent": 98.0, "resets_at": soon}),
        json!({"label": "Weekly", "used_percent": 30.0}),
    ], true, 1);
    assert_eq!(headroom_penalty(AgentKind::Codex, None), -6.0);
}

#[test]
fn regression_headroom_ignores_expired_weekly_window() {
    let (temp, _home, _cache) = isolated();
    let expired = (Utc::now() - Duration::minutes(5)).to_rfc3339();
    write_snapshot(&temp, "codex", vec![
        json!({"label": "Weekly", "used_percent": 100.0, "resets_at": expired}),
    ], true, 1);
    assert_eq!(headroom_penalty(AgentKind::Codex, None), 0.0);
}

#[test]
fn regression_headroom_ignores_expired_nearly_exhausted_short_window() {
    let (temp, _home, _cache) = isolated();
    let expired = (Utc::now() - Duration::minutes(5)).to_rfc3339();
    write_snapshot(&temp, "codex", vec![
        json!({"label": "5h", "used_percent": 99.0, "resets_at": expired}),
    ], true, 1);
    assert_eq!(headroom_penalty(AgentKind::Codex, None), 0.0);
}

#[test]
fn regression_headroom_keeps_future_weekly_window() {
    let (temp, _home, _cache) = isolated();
    let future = (Utc::now() + Duration::hours(5)).to_rfc3339();
    write_snapshot(&temp, "codex", vec![
        json!({"label": "Weekly", "used_percent": 85.0, "resets_at": future}),
    ], true, 1);
    assert_eq!(headroom_penalty(AgentKind::Codex, None), -3.0);
}

#[test]
fn selection_headroom_short_undated_window_does_not_hide_weekly_penalty() {
    let (temp, _home, _cache) = isolated();
    write_snapshot(&temp, "codex", vec![
        json!({"label": "5h", "used_percent": 85.0, "resets_at": null}),
        json!({"label": "Weekly", "used_percent": 30.0, "resets_at": null}),
    ], true, 1);
    assert_eq!(headroom_penalty(AgentKind::Codex, None), 0.0);

    write_snapshot(&temp, "codex", vec![
        json!({"label": "5h", "used_percent": 85.0, "resets_at": null}),
        json!({"label": "Weekly", "used_percent": 80.0, "resets_at": null}),
    ], true, 1);
    assert_eq!(headroom_penalty(AgentKind::Codex, None), -3.0);
}

#[test]
fn selection_headroom_stale_or_failed_probe_yields_zero() {
    let (temp, _home, _cache) = isolated();
    let windows = vec![json!({"label": "Weekly", "used_percent": 99.0})];
    write_snapshot(&temp, "codex", windows.clone(), true, 20);
    assert_eq!(headroom_penalty(AgentKind::Codex, None), 0.0);
    write_snapshot(&temp, "codex", windows, false, 1);
    assert_eq!(headroom_penalty(AgentKind::Codex, None), 0.0);
}
