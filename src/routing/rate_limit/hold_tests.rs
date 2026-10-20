// Tests for what ends a rate-limit hold: a stated time, a person, or a short
// cooldown. Every refusal string below is verbatim captured CLI output — the
// fixtures under tests/fixtures/ are copies of live ~/.aid markers.

use super::*;
use chrono::{Datelike, Duration, Local, Timelike};
use crate::types::AgentKind;

#[path = "hold_test_support.rs"]
mod helpers;
use helpers::*;

fn isolated() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(temp.path().join(".aid")).expect("aid dir");
    temp
}

/// Hold tests pin marker classification. An empty aidbar cache so a live
/// snapshot cannot release an aged Windowed marker.
fn isolate_cache(temp: &tempfile::TempDir) -> crate::routing::quota::live::CacheDirGuard {
    let cache = temp.path().join("aidbar");
    std::fs::create_dir_all(&cache).expect("cache");
    crate::routing::quota::live::CacheDirGuard::set(&cache)
}

/// Write a probe snapshot so `stored_hold` sees a live quota source for this
/// route. The window sits at 100% so `snapshot_overrides` does not release the
/// hold — the probe exists to keep Windowed classification, not to clear it.
fn write_probe_held(cache_dir: &std::path::Path, provider: &str) {
    std::fs::write(
        cache_dir.join(format!("{provider}.json")),
        format!(
            r#"{{"ok":true,"snapshot":{{"provider":"{provider}","windows":[{{"label":"window","used_percent":100.0,"resets_at":"2099-01-01T00:00:00Z","group":null}}],"fetched_at":"2099-01-01T00:00:00Z"}}}}"#
        ),
    )
    .expect("cache");
}

/// The refusal aid could not classify at all: a bare status code caught on
/// stderr, matching no signature. It writes a marker with no recovery time and
/// no manual hold, and that must expire on its own.
///
/// Treating "no stated recovery time" as "never recovers" blackholed a route
/// until someone ran `aid config clear-limit` — the same defect as an outage
/// going unrecorded, pointing the other way.
#[test]
fn a_transient_refusal_with_no_stated_time_expires_on_its_own() {
    let temp = isolated();
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

    mark_rate_limited(&AgentKind::Claude, None, "HTTP 429 Too Many Requests");

    let info = get_rate_limit_info(&AgentKind::Claude, None).expect("marker written");
    assert_eq!(info.recovery_at, None, "no time was stated, so none is invented");
    assert!(!info.needs_human, "a bare 429 does not need a person");
    assert!(
        !is_rate_limited(&AgentKind::Claude, None),
        "Transient cooldown is not Held — scoring and dispatch stay open"
    );

    // Age the marker past the cooldown window: the route is tried again.
    age_marker(&marker_path(&AgentKind::Claude, None), RATE_LIMIT_WINDOW_SECS + 60);
    assert!(
        !is_rate_limited(&AgentKind::Claude, None),
        "a transient refusal must not hold a route open indefinitely"
    );
}

/// Windowed grok is not released by elapsed time — only a dated snapshot or
/// `aid config clear-limit` ends it. A probe is written so the route keeps its
/// Windowed class; the probe's window sits at 100% so it does not release.
#[test]
fn a_windowed_grok_hold_is_not_released_by_elapsed_time() {
    let temp = isolated();
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());
    let _cache = isolate_cache(&temp);
    write_probe_held(&temp.path().join("aidbar"), "grok");

    mark_rate_limited(&AgentKind::Grok, None, "API error (status 402 Payment Required): Grok Build usage balance exhausted");

    let info = get_rate_limit_info(&AgentKind::Grok, None).expect("marker written");
    assert_eq!(info.recovery_at, None, "no invented reset time");
    assert!(!info.needs_human, "Windowed is not a person hold");

    age_marker(&marker_path(&AgentKind::Grok, None), RATE_LIMIT_WINDOW_SECS * 100);
    assert!(
        is_rate_limited(&AgentKind::Grok, None),
        "a Windowed hold must survive any amount of elapsed time"
    );
    assert!(dispatch_blocking_hold(&AgentKind::Grok, None).is_some());

    assert!(clear_rate_limit(&AgentKind::Grok, None));
    assert!(!is_rate_limited(&AgentKind::Grok, None));
}

/// Every refusal we classify as needing a person, with the message that made us
/// classify it that way. Each holds with no recovery time rather than being
/// given one that later expires.
#[test]
fn every_human_ended_refusal_holds_without_an_invented_reset_time() {
    let temp = isolated();
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

    for (agent, message) in [
        (AgentKind::Copilot, "You have exceeded your monthly quota"),
        (
            AgentKind::Copilot,
            "You've reached your premium request limit for this billing cycle.",
        ),
        (
            AgentKind::OpenCode,
            "Insufficient balance. Manage your billing here: https://opencode.ai/",
        ),
        (AgentKind::Droid, "402 payment required: reload your tokens"),
        (
            AgentKind::Oz,
            "Error: Your credentials are invalid. Please log in again with `oz login`.",
        ),
    ] {
        clear_rate_limit(&agent, None);
        mark_rate_limited(&agent, None, message);
        let info = get_rate_limit_info(&agent, None).expect("marker written");
        assert_eq!(
            info.recovery_at, None,
            "{agent:?} must not be given a reset time it never stated: {message}"
        );
        assert!(info.needs_human, "{agent:?} must be held for a person: {message}");

        age_marker(&marker_path(&agent, None), RATE_LIMIT_WINDOW_SECS * 100);
        assert!(is_rate_limited(&agent, None), "{agent:?} hold must survive elapsed time");
        clear_rate_limit(&agent, None);
    }
}

/// The clock-ended class keeps its bounded cooldown. Holding these for a person
/// would strand a route that comes back by itself.
#[test]
fn clock_ended_refusals_still_get_a_recovery_time() {
    let temp = isolated();
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

    for (agent, message) in [
        (AgentKind::Codex, "You have hit your usage limit."),
        (AgentKind::Oz, "Error: Quota limit reached."),
        (AgentKind::Cursor, "quota exceeded for this workspace"),
        (
            AgentKind::Antigravity,
            "Individual quota reached. Please upgrade your subscription to increase \
             your limits. Resets in 59m21s.",
        ),
        (
            AgentKind::Droid,
            "402 You've reached your weekly standard usage limit (resets in 1 day).",
        ),
    ] {
        clear_rate_limit(&agent, None);
        mark_rate_limited(&agent, None, message);
        let info = get_rate_limit_info(&agent, None).expect("marker written");
        assert!(
            info.recovery_at.is_some(),
            "{agent:?} recovers on a clock and must say when: {message}"
        );
        assert!(!info.needs_human, "{agent:?} must not wait for a person: {message}");
        clear_rate_limit(&agent, None);
    }
}

/// A stated reset time outranks the signature's class default: a refusal that
/// does name its reset date is held to that date, not to a person.
#[test]
fn a_stated_reset_time_wins_over_the_class_default() {
    let temp = isolated();
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

    mark_rate_limited(&AgentKind::Droid, None, "402 payment required: reload your tokens (resets in 2 hours)");
    let info = get_rate_limit_info(&AgentKind::Droid, None).expect("marker written");
    assert!(info.recovery_at.is_some(), "the stated time must be recorded");
    assert!(!info.needs_human, "a stated time is not a human hold");
}

/// A recovery phrase we cannot parse is not a permanent hold either. Writing
/// "tomorrow morning" into the field and then failing to read it back must fall
/// to the bounded cooldown, not to "out forever".
#[test]
fn an_unparseable_recovery_phrase_falls_back_to_the_cooldown() {
    let temp = isolated();
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

    let path = marker_path(&AgentKind::Codex, None);
    std::fs::write(&path, "recovery_at: tomorrow morning\nmessage: out of quota\n")
        .expect("write marker");

    assert!(
        !is_rate_limited(&AgentKind::Codex, None),
        "unparseable recovery is Transient, and Transient is not Held"
    );
    age_marker(&path, RATE_LIMIT_WINDOW_SECS + 60);
    assert!(
        !is_rate_limited(&AgentKind::Codex, None),
        "an unreadable time must expire, not become permanent"
    );
}

/// The live markers copied from ~/.aid on 2026-08-07. Each states its reset time
/// in its own provider's phrasing, and every one of those must still read as out —
/// this fix must not shorten a hold that exists today.
///
/// The stated time is rebuilt one day from now before each marker is written.
/// The phrasing each provider uses is the part under test —
/// `parse_recovery_datetime` has to read `Aug 11th, 2026 2:23 PM` and
/// `Aug 08, 2026 12:39 AM` alike — so the fixture's formatting is preserved.
///
/// The marker is then aged past the transient cooldown. Without that, a `recovery_at`
/// this parser failed to read would fall through to `StoredHold::Transient`, and a
/// freshly written file is inside its window, so the assertion would pass without the
/// stated time doing any work at all.
#[test]
fn live_markers_with_a_future_reset_time_still_read_as_out() {
    let temp = isolated();
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

    for (agent, fixture) in [
        (AgentKind::Codex, "rate-limit-codex"),
                (AgentKind::Droid, "rate-limit-droid"),
        (AgentKind::OpenCode, "rate-limit-opencode"),
    ] {
        let content = with_relative_recovery_time(&read_fixture(fixture), Duration::days(1));
        let path = marker_path(&agent, None);
        std::fs::write(&path, &content).expect("write marker");
        age_marker(&path, RATE_LIMIT_WINDOW_SECS + 60);
        assert!(
            is_rate_limited(&agent, None),
            "{fixture} states a future reset time and must still hold"
        );
    }
}

/// oz's live marker had a reset time in the past when it was captured. `aid config
/// agents` printed it as the current status because the renderer never asked
/// whether the hold was over.
#[test]
fn a_marker_whose_stated_time_has_passed_reads_as_recovered() {
    let temp = isolated();
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

    let content = with_relative_recovery_time(&read_fixture("rate-limit-oz"), Duration::minutes(-1));
    std::fs::write(marker_path(&AgentKind::Oz, None), content)
        .expect("write marker");
    assert!(
        !is_rate_limited(&AgentKind::Oz, None),
        "a reset time in the past means the route is available again"
    );
}

/// A marker written before the hold classes existed has no `hold:` line. It must
/// not be read as permanent — that is the blackhole this fix removes.

#[path = "hold_tests_more_1.rs"]
mod more;
