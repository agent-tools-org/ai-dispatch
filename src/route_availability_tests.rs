// PR-1 override policy: clock release, NeedsHuman veto, Windowed dated arm.
// Frozen stored_hold order: Windowed before hold: manual; unmatched manual stays NeedsHuman.

use super::*;
use crate::paths::AidHomeGuard;
use crate::rate_limit::{self, dispatch_blocking_hold, format_hold_end, mark_rate_limited};
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
    let held = apply_hold(
        &AgentKind::Grok,
        None,
        None,
        hold.clone(),
        QuotaWall::Windowed,
        None,
        Some(mtime),
        Some(undated.clone()),
    );
    assert_eq!(held.status, RouteStatus::Held);
    assert_eq!(held.ends, HoldEnd::SnapshotDatedWindow);

    let dated = probe("grok", &[0.0], true, fetched, true);
    assert!(snapshot_overrides(&hold, &dated, mtime, &dated.windows));
    let freed = apply_hold(
        &AgentKind::Grok,
        None,
        None,
        hold.clone(),
        QuotaWall::Windowed,
        None,
        Some(mtime),
        Some(dated),
    );
    assert_eq!(freed.status, RouteStatus::Dispatchable);
    assert!(!format_hold_end_for(&hold, &AgentKind::Grok, None, None).contains("cooling down"));
}

#[test]
fn snapshot_overrides_rejects_equal_older_or_mixed_windows() {
    let mtime = SystemTime::now();
    let hold = StoredHold::Until(future_clock().0);

    let equal = probe("codex", &[0.0], false, DateTime::<Utc>::from(mtime), true);
    assert!(!snapshot_overrides(&hold, &equal, mtime, &equal.windows));

    let older_at = DateTime::<Utc>::from(mtime - Duration::from_secs(1));
    let older = probe("codex", &[0.0], false, older_at, true);
    assert!(!snapshot_overrides(&hold, &older, mtime, &older.windows));

    let mixed = probe("codex", &[0.0, 100.0], false, newer_than(mtime), true);
    assert!(!snapshot_overrides(&hold, &mixed, mtime, &mixed.windows));
}

#[test]
fn format_hold_end_classifies_the_full_grok_fixture() {
    let temp = tempfile::tempdir().expect("temp");
    let _home = AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(temp.path().join(".aid")).expect("aid dir");

    let content = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rate-limit-grok"),
    )
    .expect("grok fixture");
    let first = content.lines().find_map(|l| l.strip_prefix("message: ")).unwrap_or("");
    assert!(!first.contains("usage balance exhausted"));
    assert!(!matches!(
        stored_hold(&content, &AgentKind::Grok),
        StoredHold::Transient
    ));

    std::fs::write(rate_limit::marker_path(&AgentKind::Grok, None), &content).expect("marker");
    let info = rate_limit::get_rate_limit_info(&AgentKind::Grok, None).expect("info");
    assert!(info.marker.contains("usage balance exhausted"));
    let end = format_hold_end(&AgentKind::Grok, None, &info);
    assert!(
        !end.contains("cooling down"),
        "full fixture must not print cooling down, got {end:?}"
    );

    // Simulate post-PR-3: needs_human is false; reconstruction from the first
    // message: line would be Transient. Original bytes must still classify.
    let mut flipped = info;
    flipped.needs_human = false;
    assert!(!format_hold_end(&AgentKind::Grok, None, &flipped).contains("cooling down"));
}
