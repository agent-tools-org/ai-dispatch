// Tests for what ends a rate-limit hold: a stated time, a person, or a short
// cooldown. Every refusal string below is verbatim captured CLI output — the
// fixtures under tests/fixtures/ are copies of live ~/.aid markers.

use super::*;
use crate::types::AgentKind;

fn isolated() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(temp.path().join(".aid")).expect("aid dir");
    temp
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
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    mark_rate_limited(&AgentKind::Claude, "HTTP 429 Too Many Requests");

    let info = get_rate_limit_info(&AgentKind::Claude).expect("marker written");
    assert_eq!(info.recovery_at, None, "no time was stated, so none is invented");
    assert!(!info.needs_human, "a bare 429 does not need a person");
    assert!(is_rate_limited(&AgentKind::Claude), "still inside the cooldown");

    // Age the marker past the cooldown window: the route is tried again.
    age_marker(&marker_path(&AgentKind::Claude), RATE_LIMIT_WINDOW_SECS + 60);
    assert!(
        !is_rate_limited(&AgentKind::Claude),
        "a transient refusal must not hold a route open indefinitely"
    );
}

/// The other class. A spent balance does not come back on a clock, so ageing
/// the marker changes nothing — only `aid config clear-limit` does.
#[test]
fn a_refusal_that_needs_a_person_is_not_released_by_time() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    mark_rate_limited(
        &AgentKind::Grok,
        "API error (status 402 Payment Required): Grok Build usage balance exhausted",
    );

    let info = get_rate_limit_info(&AgentKind::Grok).expect("marker written");
    assert_eq!(info.recovery_at, None, "no invented reset time");
    assert!(info.needs_human);

    age_marker(&marker_path(&AgentKind::Grok), RATE_LIMIT_WINDOW_SECS * 100);
    assert!(
        is_rate_limited(&AgentKind::Grok),
        "a spent balance must survive any amount of elapsed time"
    );

    assert!(clear_rate_limit(&AgentKind::Grok));
    assert!(!is_rate_limited(&AgentKind::Grok));
}

/// Every refusal we classify as needing a person, with the message that made us
/// classify it that way. Each holds with no recovery time rather than being
/// given one that later expires.
#[test]
fn every_human_ended_refusal_holds_without_an_invented_reset_time() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    for (agent, message) in [
        (
            AgentKind::Cursor,
            "ActionRequiredError: Increase limits for faster responses You're out of usage. \
             Switch to Auto, or ask your admin to increase your limit to continue.",
        ),
        (AgentKind::Copilot, "You have exceeded your monthly quota"),
        (
            AgentKind::Copilot,
            "You've reached your premium request limit for this billing cycle.",
        ),
        (
            AgentKind::Grok,
            "API error (status 402 Payment Required): Grok Build usage balance exhausted",
        ),
        (
            AgentKind::OpenCode,
            "Insufficient balance. Manage your billing here: https://opencode.ai/",
        ),
        (AgentKind::Droid, "402 payment required: reload your tokens"),
        (
            AgentKind::Gemini,
            "IneligibleTierError: This client is no longer supported for Gemini Code \
             Assist for individuals",
        ),
    ] {
        clear_rate_limit(&agent);
        mark_rate_limited(&agent, message);
        let info = get_rate_limit_info(&agent).expect("marker written");
        assert_eq!(
            info.recovery_at, None,
            "{agent:?} must not be given a reset time it never stated: {message}"
        );
        assert!(info.needs_human, "{agent:?} must be held for a person: {message}");

        age_marker(&marker_path(&agent), RATE_LIMIT_WINDOW_SECS * 100);
        assert!(is_rate_limited(&agent), "{agent:?} hold must survive elapsed time");
        clear_rate_limit(&agent);
    }
}

/// The clock-ended class keeps its bounded cooldown. Holding these for a person
/// would strand a route that comes back by itself.
#[test]
fn clock_ended_refusals_still_get_a_recovery_time() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    for (agent, message) in [
        (AgentKind::Qwen, "Quota exhausted: Your token-plan 5-hour quota has been exhausted."),
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
        clear_rate_limit(&agent);
        mark_rate_limited(&agent, message);
        let info = get_rate_limit_info(&agent).expect("marker written");
        assert!(
            info.recovery_at.is_some(),
            "{agent:?} recovers on a clock and must say when: {message}"
        );
        assert!(!info.needs_human, "{agent:?} must not wait for a person: {message}");
        clear_rate_limit(&agent);
    }
}

/// A stated reset time outranks the signature's class default: a refusal that
/// does name its reset date is held to that date, not to a person.
#[test]
fn a_stated_reset_time_wins_over_the_class_default() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    mark_rate_limited(
        &AgentKind::Droid,
        "402 payment required: reload your tokens (resets in 2 hours)",
    );
    let info = get_rate_limit_info(&AgentKind::Droid).expect("marker written");
    assert!(info.recovery_at.is_some(), "the stated time must be recorded");
    assert!(!info.needs_human, "a stated time is not a human hold");
}

/// A recovery phrase we cannot parse is not a permanent hold either. Writing
/// "tomorrow morning" into the field and then failing to read it back must fall
/// to the bounded cooldown, not to "out forever".
#[test]
fn an_unparseable_recovery_phrase_falls_back_to_the_cooldown() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    let path = marker_path(&AgentKind::Qwen);
    std::fs::write(&path, "recovery_at: tomorrow morning\nmessage: out of quota\n")
        .expect("write marker");

    assert!(is_rate_limited(&AgentKind::Qwen), "fresh marker still holds");
    age_marker(&path, RATE_LIMIT_WINDOW_SECS + 60);
    assert!(
        !is_rate_limited(&AgentKind::Qwen),
        "an unreadable time must expire, not become permanent"
    );
}

/// The live markers copied from ~/.aid on 2026-08-07. The three that state a
/// future reset time must still read as out — this fix must not shorten a hold
/// that exists today.
#[test]
fn live_markers_with_a_future_reset_time_still_read_as_out() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    for (agent, fixture) in [
        (AgentKind::Codex, "rate-limit-codex"),
        (AgentKind::Qwen, "rate-limit-qwen"),
        (AgentKind::Droid, "rate-limit-droid"),
        (AgentKind::OpenCode, "rate-limit-opencode"),
    ] {
        let content = read_fixture(fixture);
        std::fs::write(marker_path(&agent), &content).expect("write marker");
        assert!(
            is_rate_limited(&agent),
            "{fixture} states a future reset time and must still hold"
        );
    }
}

/// oz's live marker says "try again at Aug 07, 2026 02:05 AM" — fourteen hours
/// in the past when it was captured. `aid config agents` printed it as the
/// current status because the renderer never asked whether the hold was over.
#[test]
fn a_marker_whose_stated_time_has_passed_reads_as_recovered() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    std::fs::write(marker_path(&AgentKind::Oz), read_fixture("rate-limit-oz"))
        .expect("write marker");
    assert!(
        !is_rate_limited(&AgentKind::Oz),
        "a reset time in the past means the route is available again"
    );
}

/// A marker written before the hold classes existed has no `hold:` line. It must
/// not be read as permanent — that is the blackhole this fix removes.
#[test]
fn a_marker_without_a_hold_field_is_not_permanent() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    let path = marker_path(&AgentKind::Copilot);
    std::fs::write(&path, "recovery_at: \nmessage: some old refusal\n").expect("write marker");
    age_marker(&path, RATE_LIMIT_WINDOW_SECS + 60);
    assert!(!is_rate_limited(&AgentKind::Copilot));
}

/// Group markers are held by the same three classes as agent markers, so a
/// spent cursor premium pool survives while `auto` is untouched.
#[test]
fn a_group_marker_holds_for_a_person_too() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    let cursor = AgentKind::Cursor;
    mark_group_rate_limited(
        &cursor,
        "premium",
        "ActionRequiredError: Increase limits for faster responses You're out of usage. \
         Switch to Auto, or ask your admin to increase your limit to continue.",
    );

    age_marker(&group_marker_path(&cursor, "premium"), RATE_LIMIT_WINDOW_SECS * 100);
    assert!(is_group_rate_limited(&cursor, "premium"));
    assert!(!is_group_rate_limited(&cursor, "auto"), "auto keeps serving");
    assert!(!is_rate_limited(&cursor), "the agent itself is not written off");

    assert!(clear_all_rate_limits_for_agent(&cursor));
    assert!(!is_group_rate_limited(&cursor, "premium"));
}

fn read_fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {path:?}: {err}"))
}

/// Backdate a marker's mtime so the bounded cooldown can be exercised without
/// sleeping. The transient class measures from the write time.
fn age_marker(path: &std::path::Path, seconds: u64) {
    let when = std::time::SystemTime::now() - std::time::Duration::from_secs(seconds);
    let file = std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap_or_else(|err| panic!("open {path:?}: {err}"));
    file.set_modified(when)
        .unwrap_or_else(|err| panic!("set mtime on {path:?}: {err}"));
}
