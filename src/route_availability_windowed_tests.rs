// PR-3 live seams: Windowed grok/cursor release; prepaid and plan-change stay held.

use super::*;
use crate::paths::AidHomeGuard;
use crate::rate_limit::{
    dispatch_blocking_hold, dispatch_blocking_hold_for_model, format_hold_end, get_rate_limit_info,
    is_group_rate_limited, is_rate_limited, mark_rate_limited, mark_rate_limited_for_message,
    marker_path,
};
use crate::types::AgentKind;
use std::path::Path;

const GROK_402: &str =
    "API error (status 402 Payment Required): Grok Build usage balance exhausted";
const CURSOR_PREMIUM: &str = "ActionRequiredError: Increase limits for faster responses You're out of usage. \
     Switch to Auto, or ask your admin to increase your limit to continue.";
const OPENCODE_PREPAID: &str = "Insufficient balance. Manage your billing here: https://opencode.ai/";
const GEMINI_TIER: &str = "IneligibleTierError: This client is no longer supported for Gemini Code \
     Assist for individuals";
const DATED: &str = "2026-08-18T00:55:28Z";

struct Isolated {
    _temp: tempfile::TempDir,
    _home: AidHomeGuard,
    _cache: crate::live_quota::CacheDirGuard,
    cache_dir: std::path::PathBuf,
}

fn isolated() -> Isolated {
    let temp = tempfile::tempdir().expect("temp");
    std::fs::create_dir_all(temp.path().join(".aid")).expect("aid dir");
    let home = AidHomeGuard::set(temp.path());
    let cache_dir = temp.path().join("aidbar");
    std::fs::create_dir_all(&cache_dir).expect("cache");
    let cache = crate::live_quota::CacheDirGuard::set(&cache_dir);
    Isolated {
        _temp: temp,
        _home: home,
        _cache: cache,
        cache_dir,
    }
}

fn write_cache(dir: &Path, provider: &str, windows: &str) {
    std::fs::write(
        dir.join(format!("{provider}.json")),
        format!(
            r#"{{"ok":true,"snapshot":{{"provider":"{provider}","windows":[{windows}],"fetched_at":"2099-01-01T00:00:00Z"}}}}"#
        ),
    )
    .expect("cache");
}

fn window(label: &str, used: f64, resets: Option<&str>, group: Option<&str>) -> String {
    let resets_at = match resets {
        Some(at) => format!("\"{at}\""),
        None => "null".to_string(),
    };
    let group = match group {
        Some(name) => format!("\"{name}\""),
        None => "null".to_string(),
    };
    format!(
        r#"{{"label":"{label}","used_percent":{used},"resets_at":{resets_at},"group":{group}}}"#
    )
}

fn grok_fixture() -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rate-limit-grok"),
    )
    .expect("grok fixture")
}

fn incident_marker() -> String {
    format!("recovery_at:\nhold: manual\nprovider: unknown\nmessage: {GROK_402}\n")
}

/// A probe is written so the route keeps its Windowed class — the signature
/// assumes aidbar maps the billing period to a dated resets_at. Without a probe
/// the recovery is unobservable and the hold becomes human-cleared (see
/// `rate_limit_hold_tests::a_probeless_windowed_refusal_states_a_human_clear`).
fn write_grok_probe(iso: &Isolated) {
    write_cache(
        &iso.cache_dir,
        "grok",
        &window("Aug 11 – Aug 18", 100.0, Some(DATED), None),
    );
}

#[test]
fn incident_marker_classifies_as_windowed_never_needs_human() {
    let iso = isolated();
    write_grok_probe(&iso);
    for content in [incident_marker(), grok_fixture()] {
        assert!(
            matches!(stored_hold(&content, &AgentKind::Grok), StoredHold::Windowed),
            "incident bytes must re-read as Windowed: {content}"
        );
        assert!(!matches!(
            stored_hold(&content, &AgentKind::Grok),
            StoredHold::NeedsHuman
        ));
    }
}

#[test]
fn grok_dated_zero_percent_newer_snapshot_unblocks_dispatch() {
    let iso = isolated();
    std::fs::write(marker_path(&AgentKind::Grok, None), incident_marker()).expect("marker");
    write_cache(
        &iso.cache_dir,
        "grok",
        &window("Aug 11 – Aug 18", 0.0, Some(DATED), None),
    );
    assert!(dispatch_blocking_hold(&AgentKind::Grok, None).is_none());
}

#[test]
fn grok_undated_zero_percent_stays_held() {
    let iso = isolated();
    std::fs::write(marker_path(&AgentKind::Grok, None), incident_marker()).expect("marker");
    write_cache(
        &iso.cache_dir,
        "grok",
        &window("Aug 11 – Aug 18", 0.0, None, None),
    );
    assert!(dispatch_blocking_hold(&AgentKind::Grok, None).is_some());
}

#[test]
fn grok_fixture_format_hold_end_is_not_cooling_down() {
    let iso = isolated();
    write_grok_probe(&iso);
    std::fs::write(marker_path(&AgentKind::Grok, None), grok_fixture()).expect("marker");
    let info = get_rate_limit_info(&AgentKind::Grok, None).expect("info");
    let end = format_hold_end(&AgentKind::Grok, None, &info);
    assert!(!end.contains("cooling down"), "got {end:?}");
    assert!(end.contains("dated grok snapshot"), "got {end:?}");
}

#[test]
fn cursor_premium_plan_zero_dated_releases_despite_ondemand_overage() {
    let iso = isolated();
    mark_rate_limited_for_message(&AgentKind::Cursor, None, CURSOR_PREMIUM);
    write_cache(
        &iso.cache_dir,
        "cursor",
        &format!(
            "{},{}",
            window("Plan", 0.0, Some(DATED), None),
            window("On-demand", 115.0, Some(DATED), None)
        ),
    );
    assert!(
        dispatch_blocking_hold_for_model(&AgentKind::Cursor, None, Some("composer-2.5")).is_none()
    );
    assert!(!is_rate_limited(&AgentKind::Cursor, None));
    assert!(!is_group_rate_limited(&AgentKind::Cursor, None, "premium"));
}

#[test]
fn cursor_premium_plan_undated_stays_group_held() {
    let iso = isolated();
    mark_rate_limited_for_message(&AgentKind::Cursor, None, CURSOR_PREMIUM);
    write_cache(
        &iso.cache_dir,
        "cursor",
        &format!(
            "{},{}",
            window("Plan", 0.0, None, None),
            window("On-demand", 0.0, Some(DATED), None)
        ),
    );
    assert!(is_group_rate_limited(&AgentKind::Cursor, None, "premium"));
    assert!(
        dispatch_blocking_hold_for_model(&AgentKind::Cursor, None, Some("composer-2.5")).is_some()
    );
}

#[test]
fn cursor_premium_plan_one_hundred_dated_stays_group_held() {
    let iso = isolated();
    mark_rate_limited_for_message(&AgentKind::Cursor, None, CURSOR_PREMIUM);
    write_cache(
        &iso.cache_dir,
        "cursor",
        &window("Plan", 100.0, Some(DATED), None),
    );
    assert!(is_group_rate_limited(&AgentKind::Cursor, None, "premium"));
}

#[test]
fn cursor_group_field_wins_over_plan_label() {
    let iso = isolated();
    mark_rate_limited_for_message(&AgentKind::Cursor, None, CURSOR_PREMIUM);
    write_cache(
        &iso.cache_dir,
        "cursor",
        &format!(
            "{},{}",
            window("Plan", 0.0, Some(DATED), None),
            window("On-demand", 100.0, Some(DATED), Some("premium"))
        ),
    );
    assert!(
        is_group_rate_limited(&AgentKind::Cursor, None, "premium"),
        "windows[].group present must win; On-demand at 100% blocks"
    );
}

#[test]
fn opencode_prepaid_stays_held_dated_or_undated() {
    let iso = isolated();
    mark_rate_limited(&AgentKind::OpenCode, None, OPENCODE_PREPAID);
    for resets in [None, Some(DATED)] {
        write_cache(
            &iso.cache_dir,
            "opencode",
            &window("5h", 0.0, resets, None),
        );
        assert!(
            dispatch_blocking_hold(&AgentKind::OpenCode, None).is_some(),
            "prepaid must stay held with resets_at={resets:?}"
        );
    }
}

#[test]
fn gemini_ineligible_tier_stays_held_with_any_snapshot() {
    let iso = isolated();
    mark_rate_limited(&AgentKind::Gemini, None, GEMINI_TIER);
    write_cache(&iso.cache_dir, "grok", &window("pool", 0.0, Some(DATED), None));
    assert!(dispatch_blocking_hold(&AgentKind::Gemini, None).is_some());
}

#[test]
fn agy_group_hold_without_group_field_fails_closed() {
    let iso = isolated();
    crate::rate_limit::mark_group_rate_limited(
        &AgentKind::Antigravity,
        None,
        "claude",
        "Individual quota reached. Please upgrade your subscription to increase your limits. Resets in 59m21s.",
    );
    write_cache(
        &iso.cache_dir,
        "agy",
        &window("Claude and GPT models 5h", 0.0, Some(DATED), None),
    );
    assert!(is_group_rate_limited(
        &AgentKind::Antigravity,
        None,
        "claude"
    ));
    assert!(
        dispatch_blocking_hold_for_model(&AgentKind::Antigravity, None, Some("claude-sonnet-4-6"))
            .is_some()
    );
}
