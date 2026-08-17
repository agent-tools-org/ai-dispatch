// PR-1 override policy: clock release, NeedsHuman veto, Windowed dated arm.
// Frozen stored_hold order: Windowed before hold: manual; unmatched manual stays NeedsHuman.

use super::*;
use crate::paths::AidHomeGuard;
use crate::rate_limit::{self, dispatch_blocking_hold, mark_rate_limited};
use crate::types::AgentKind;
use chrono::{Duration as ChronoDuration, Local, Utc};
use std::time::{Duration, SystemTime};

fn future_clock() -> (chrono::NaiveDateTime, String) {
    let at = Local::now().naive_local() + ChronoDuration::hours(2);
    (at, at.format("%b %d, %Y %I:%M %p").to_string())
}

fn clock_marker(recovery_at: &str) -> String {
    format!("recovery_at: {recovery_at}\nmessage: hit your usage limit\n")
}

fn probe(
    provider: &str,
    used: &[f64],
    dated: bool,
    fetched_at: DateTime<Utc>,
    ok: bool,
) -> ProbeEvidence {
    ProbeEvidence {
        provider: provider.to_string(),
        fetched_at,
        age: Duration::from_secs(0),
        stale: false,
        ok,
        windows: used
            .iter()
            .map(|percent| WindowView {
                label: "window".to_string(),
                used_percent: *percent,
                resets_at: dated.then_some(fetched_at + ChronoDuration::days(1)),
                group: None,
            })
            .collect(),
    }
}

fn newer_than(mtime: SystemTime) -> DateTime<Utc> {
    DateTime::<Utc>::from(mtime + Duration::from_secs(1))
}

#[test]
fn clock_hold_newer_snapshot_with_headroom_is_dispatchable() {
    let (at, stated) = future_clock();
    let mtime = SystemTime::now() - Duration::from_secs(60);
    let snap = probe("codex", &[0.0], false, newer_than(mtime), true);
    let avail = decide(
        &AgentKind::Codex,
        None,
        None,
        Some(&clock_marker(&stated)),
        Some(mtime),
        Some(snap.clone()),
    );
    assert_eq!(avail.status, RouteStatus::Dispatchable);
    assert!(snapshot_overrides(
        &StoredHold::Until(at),
        &snap,
        mtime,
        &snap.windows
    ));
}

#[test]
fn clock_hold_at_one_hundred_percent_stays_held() {
    let (at, stated) = future_clock();
    let mtime = SystemTime::now() - Duration::from_secs(60);
    let snap = probe("codex", &[100.0], false, newer_than(mtime), true);
    let avail = decide(
        &AgentKind::Codex,
        None,
        None,
        Some(&clock_marker(&stated)),
        Some(mtime),
        Some(snap.clone()),
    );
    assert_eq!(avail.status, RouteStatus::Held);
    assert!(!snapshot_overrides(
        &StoredHold::Until(at),
        &snap,
        mtime,
        &snap.windows
    ));
}

#[test]
fn clock_hold_twenty_minute_snapshot_still_releases_dispatch() {
    let temp = tempfile::tempdir().expect("temp");
    let _home = AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(temp.path().join(".aid")).expect("aid dir");
    let aidbar = temp.path().join("aidbar");
    std::fs::create_dir_all(&aidbar).expect("cache");
    let _cache = crate::live_quota::CacheDirGuard::set(&aidbar);

    let stated = rate_limit::test_future_recovery_time();
    mark_rate_limited(
        &AgentKind::Codex,
        None,
        &format!("You have hit your usage limit. try again at {stated}."),
    );
    let marker = rate_limit::marker_path(&AgentKind::Codex, None);
    let marker_mtime = SystemTime::now() - Duration::from_secs(21 * 60);
    std::fs::File::options()
        .write(true)
        .open(&marker)
        .expect("open marker")
        .set_modified(marker_mtime)
        .expect("age marker");

    let fetched = Utc::now() - ChronoDuration::minutes(20);
    std::fs::write(
        aidbar.join("codex.json"),
        format!(
            r#"{{"ok":true,"snapshot":{{"provider":"codex","windows":[{{"label":"Weekly","used_percent":0.0,"resets_at":null}}],"fetched_at":"{}"}}}}"#,
            fetched.to_rfc3339()
        ),
    )
    .expect("cache");

    assert!(
        dispatch_blocking_hold(&AgentKind::Codex, None).is_none(),
        "override has no age cap when the snapshot is still newer than the marker"
    );
    assert_eq!(
        availability(&AgentKind::Codex, None).status,
        RouteStatus::Dispatchable
    );
}

#[test]
fn needs_human_veto_ignores_zero_percent_snapshot() {
    let content =
        "recovery_at:\nhold: manual\nmessage: Insufficient balance. Manage your billing here\n";
    assert!(matches!(
        stored_hold(content, &AgentKind::OpenCode),
        StoredHold::NeedsHuman
    ));
    let mtime = SystemTime::now() - Duration::from_secs(60);
    let snap = probe("opencode", &[0.0], true, newer_than(mtime), true);
    assert!(!snapshot_overrides(
        &StoredHold::NeedsHuman,
        &snap,
        mtime,
        &snap.windows
    ));
    let avail = decide(
        &AgentKind::OpenCode,
        None,
        None,
        Some(content),
        Some(mtime),
        Some(snap),
    );
    assert_eq!(avail.status, RouteStatus::Held);
    assert_eq!(avail.wall, QuotaWall::Prepaid);
}

#[test]
fn unmatched_manual_hold_stays_needs_human_not_transient() {
    let content = "recovery_at:\nhold: manual\nmessage: handwritten note with no needle\n";
    assert!(
        matches!(
            stored_hold(content, &AgentKind::Claude),
            StoredHold::NeedsHuman
        ),
        "unmatched hold: manual must not collapse to Transient"
    );
    assert_eq!(wall_of(&AgentKind::Claude, content), QuotaWall::Prepaid);
}

#[test]
fn unmapped_empty_or_failed_snapshot_does_not_override() {
    let mtime = SystemTime::now() - Duration::from_secs(60);
    let fetched = newer_than(mtime);
    let clock = StoredHold::Until(future_clock().0);

    assert!(live_quota::snapshot(&AgentKind::Copilot).is_none());

    let empty = probe("codex", &[], false, fetched, true);
    assert!(!snapshot_overrides(&clock, &empty, mtime, &empty.windows));

    let failed = probe("codex", &[0.0], false, fetched, false);
    assert!(!snapshot_overrides(&clock, &failed, mtime, &failed.windows));

    let avail = decide(
        &AgentKind::Copilot,
        None,
        None,
        Some("recovery_at:\nhold: manual\nmessage: exceeded your monthly quota\n"),
        Some(mtime),
        None,
    );
    assert_eq!(avail.status, RouteStatus::Held);
}

#[test]
fn windowed_arm_requires_a_dated_window() {
    let mtime = SystemTime::now() - Duration::from_secs(60);
    let fetched = newer_than(mtime);
    let hold = StoredHold::Windowed;

    let undated = probe("grok", &[0.0], false, fetched, true);
    assert!(
        !snapshot_overrides(&hold, &undated, mtime, &undated.windows),
        "percent alone must not release Windowed"
    );
    let held = decide(
        &AgentKind::Grok,
        None,
        None,
        Some("recovery_at:\nhold: manual\nmessage: synthetic windowed"),
        Some(mtime),
        Some(undated.clone()),
    );
    // No Windowed signature yet, so stored_hold sees unmatched manual → NeedsHuman.
    // Pin the compiled arm via the helper, not the marker text.
    assert!(!snapshot_overrides(
        &hold,
        &undated,
        mtime,
        &undated.windows
    ));
    assert_eq!(held.status, RouteStatus::Held);

    let dated = probe("grok", &[0.0], true, fetched, true);
    assert!(snapshot_overrides(&hold, &dated, mtime, &dated.windows));
    let released = released(QuotaWall::Windowed, Some(dated), Some(mtime));
    assert_eq!(released.status, RouteStatus::Dispatchable);
    assert!(!format_hold_end_for(&hold, &AgentKind::Grok, None, None).contains("cooling down"));
}
