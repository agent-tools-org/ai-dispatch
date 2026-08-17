// PR-2: every hold reader is a facade over RouteAvailability.
// Pins group-hold + windows[].group agreement, fail-closed, Transient != Held.

use super::*;
use crate::agent::model_group::healthy_model_for;
use crate::paths::AidHomeGuard;
use crate::rate_limit::{
    active_group_holds, dispatch_blocking_hold_for_model, is_group_rate_limited, is_rate_limited,
    mark_group_rate_limited, mark_rate_limited,
};
use crate::types::AgentKind;
use chrono::{Duration as ChronoDuration, Utc};

const AGY_CLOCK: &str =
    "Individual quota reached. Please upgrade your subscription to increase your limits. \
     Resets in 59m21s.";

fn isolated() -> (tempfile::TempDir, AidHomeGuard, crate::live_quota::CacheDirGuard) {
    let temp = tempfile::tempdir().expect("temp");
    let home = AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(temp.path().join(".aid")).expect("aid dir");
    let aidbar = temp.path().join("aidbar");
    std::fs::create_dir_all(&aidbar).expect("cache");
    let cache = crate::live_quota::CacheDirGuard::set(&aidbar);
    (temp, home, cache)
}

fn write_agy_snapshot(cache_dir: &std::path::Path, windows: &str) {
    std::fs::write(
        cache_dir.join("agy.json"),
        format!(
            r#"{{"ok":true,"snapshot":{{"provider":"agy","windows":[{windows}],"fetched_at":"2099-01-01T00:00:00Z"}}}}"#
        ),
    )
    .expect("cache");
}

fn claude_gpt_window() -> &'static str {
    r#"{"label":"Claude and GPT models 5h","used_percent":0.0,"resets_at":"2099-01-01T00:00:00Z","group":"claude-gpt"}"#
}

fn gemini_window() -> &'static str {
    r#"{"label":"Gemini Models Weekly","used_percent":0.0,"resets_at":"2099-01-01T00:00:00Z","group":"gemini"}"#
}

fn ungrouped_window() -> &'static str {
    r#"{"label":"Claude and GPT models 5h","used_percent":0.0,"resets_at":"2099-01-01T00:00:00Z"}"#
}

fn agy_group_answers(group: &str, model: &str) -> (RouteStatus, bool, bool, bool, Option<&'static str>) {
    let avail = availability_for_group(&AgentKind::Antigravity, None, group);
    let limited = is_group_rate_limited(&AgentKind::Antigravity, None, group);
    let blocked = dispatch_blocking_hold_for_model(&AgentKind::Antigravity, None, Some(model)).is_some();
    let listed = active_group_holds(&AgentKind::Antigravity, None)
        .iter()
        .any(|(name, _)| name == group);
    let healthy = healthy_model_for(AgentKind::Antigravity, Some(model), |g| {
        is_group_rate_limited(&AgentKind::Antigravity, None, g)
    });
    (avail.status, limited, blocked, listed, healthy)
}

#[test]
fn claude_gpt_window_agrees_across_every_hold_reader() {
    let (temp, _home, _cache) = isolated();
    let agy = AgentKind::Antigravity;
    mark_group_rate_limited(&agy, None, "claude", AGY_CLOCK);
    mark_group_rate_limited(&agy, None, "gpt-oss", AGY_CLOCK);
    write_agy_snapshot(
        &temp.path().join("aidbar"),
        &format!("{},{}", claude_gpt_window(), gemini_window()),
    );

    let (claude_status, claude_lim, claude_block, claude_listed, claude_healthy) =
        agy_group_answers("claude", "claude-sonnet-4-6");
    let (gpt_status, gpt_lim, gpt_block, gpt_listed, gpt_healthy) =
        agy_group_answers("gpt-oss", "gpt-oss-120b-medium");

    assert_eq!(claude_status, RouteStatus::Dispatchable);
    assert!(!claude_lim && !claude_block && !claude_listed);
    assert_eq!(claude_healthy, None, "current claude group is serving");
    assert_eq!(gpt_status, RouteStatus::Dispatchable);
    assert!(!gpt_lim && !gpt_block && !gpt_listed);
    assert_eq!(gpt_healthy, None, "current gpt-oss group is serving");
    assert!(!is_rate_limited(&agy, None));
}

#[test]
fn group_hold_without_window_group_stays_held() {
    let (temp, _home, _cache) = isolated();
    let agy = AgentKind::Antigravity;
    mark_group_rate_limited(&agy, None, "claude", AGY_CLOCK);
    write_agy_snapshot(&temp.path().join("aidbar"), ungrouped_window());

    let (status, limited, blocked, listed, healthy) =
        agy_group_answers("claude", "claude-sonnet-4-6");
    assert_eq!(status, RouteStatus::Held);
    assert!(limited && blocked && listed);
    assert_eq!(
        healthy,
        Some("gemini-3.1-pro-high"),
        "fail-closed claude must yield the next healthy family"
    );
}

#[test]
fn gemini_window_does_not_release_claude() {
    let (temp, _home, _cache) = isolated();
    let agy = AgentKind::Antigravity;
    mark_group_rate_limited(&agy, None, "claude", AGY_CLOCK);
    mark_group_rate_limited(&agy, None, "gemini", AGY_CLOCK);
    write_agy_snapshot(&temp.path().join("aidbar"), gemini_window());

    let (claude_status, claude_lim, _, _, _) = agy_group_answers("claude", "claude-sonnet-4-6");
    let (gemini_status, gemini_lim, gemini_block, gemini_listed, _) =
        agy_group_answers("gemini", "gemini-3.6-flash-low");
    assert_eq!(claude_status, RouteStatus::Held);
    assert!(claude_lim);
    assert_eq!(gemini_status, RouteStatus::Dispatchable);
    assert!(!gemini_lim && !gemini_block && !gemini_listed);
}

#[test]
fn claude_gpt_window_matches_both_families_without_splitting() {
    let fetched = Utc::now();
    let snap = ProbeEvidence {
        provider: "agy".into(),
        fetched_at: fetched,
        age: std::time::Duration::from_secs(0),
        stale: false,
        ok: true,
        windows: vec![WindowView {
            label: "Claude and GPT models 5h".into(),
            used_percent: 0.0,
            resets_at: Some(fetched + ChronoDuration::hours(5)),
            group: Some("claude-gpt".into()),
        }],
    };
    let claude = super::policy::relevant_windows(&snap, &AgentKind::Antigravity, Some("claude"));
    let gpt = super::policy::relevant_windows(&snap, &AgentKind::Antigravity, Some("gpt-oss"));
    let gemini = super::policy::relevant_windows(&snap, &AgentKind::Antigravity, Some("gemini"));
    assert_eq!(claude.len(), 1);
    assert_eq!(gpt.len(), 1);
    assert!(gemini.is_empty());
    assert_eq!(claude[0].group.as_deref(), Some("claude-gpt"));
    assert_eq!(gpt[0].group.as_deref(), Some("claude-gpt"));
}

#[test]
fn transient_cooldown_is_not_held() {
    let (_temp, _home, _cache) = isolated();
    mark_rate_limited(&AgentKind::Claude, None, "HTTP 429 Too Many Requests");
    assert_eq!(
        availability(&AgentKind::Claude, None).status,
        RouteStatus::Degraded
    );
    assert!(
        !is_rate_limited(&AgentKind::Claude, None),
        "Transient must not apply scoring -10"
    );
    assert!(dispatch_blocking_hold_for_model(&AgentKind::Claude, None, None).is_none());
}
