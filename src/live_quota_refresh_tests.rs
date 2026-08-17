// Tests for advise/quota aidbar refresh policy. Dispatch must never spawn.

use super::*;

#[test]
fn live_quota_refresh_disabled_by_env() {
    assert!(!refresh_allowed(Some("0")));
    assert!(refresh_allowed(None));
    assert!(refresh_allowed(Some("1")));
    assert_eq!(
        decide_refresh(Some("0"), true, &["grok"], true),
        RefreshDecision::Disabled
    );
}

#[test]
fn live_quota_refresh_stays_on_disk_without_per_id() {
    let decision = decide_refresh(None, true, &["grok", "codex"], false);
    assert_eq!(
        decision,
        RefreshDecision::StayOnDisk {
            reason: "no per-id aidbar refresh",
        }
    );
}

#[test]
fn live_quota_refresh_stays_on_disk_when_aidbar_missing() {
    assert_eq!(
        decide_refresh(None, false, &["grok"], true),
        RefreshDecision::StayOnDisk {
            reason: "aidbar not on PATH",
        }
    );
}

#[test]
fn live_quota_refresh_skips_when_cache_is_fresh() {
    assert_eq!(
        decide_refresh(None, true, &[], true),
        RefreshDecision::StayOnDisk {
            reason: "cache fresh",
        }
    );
}
