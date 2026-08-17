// Tests for what ends a rate-limit hold: a stated time, a person, or a short
// cooldown. Every refusal string below is verbatim captured CLI output — the
// fixtures under tests/fixtures/ are copies of live ~/.aid markers.

use super::*;
use chrono::{Datelike, Duration, Local, Timelike};
use crate::types::AgentKind;

fn isolated() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(temp.path().join(".aid")).expect("aid dir");
    temp
}

/// Hold tests pin marker classification. An empty aidbar cache so a live
/// snapshot cannot release an aged Windowed marker.
fn isolate_cache(temp: &tempfile::TempDir) -> crate::live_quota::CacheDirGuard {
    let cache = temp.path().join("aidbar");
    std::fs::create_dir_all(&cache).expect("cache");
    crate::live_quota::CacheDirGuard::set(&cache)
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

    mark_rate_limited(&AgentKind::Claude, None, "HTTP 429 Too Many Requests");

    let info = get_rate_limit_info(&AgentKind::Claude, None).expect("marker written");
    assert_eq!(info.recovery_at, None, "no time was stated, so none is invented");
    assert!(!info.needs_human, "a bare 429 does not need a person");
    assert!(is_rate_limited(&AgentKind::Claude, None), "still inside the cooldown");

    // Age the marker past the cooldown window: the route is tried again.
    age_marker(&marker_path(&AgentKind::Claude, None), RATE_LIMIT_WINDOW_SECS + 60);
    assert!(
        !is_rate_limited(&AgentKind::Claude, None),
        "a transient refusal must not hold a route open indefinitely"
    );
}

/// Windowed grok is not released by elapsed time — only a dated snapshot or
/// `aid config clear-limit` ends it.
#[test]
fn a_windowed_grok_hold_is_not_released_by_elapsed_time() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());
    let _cache = isolate_cache(&temp);

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
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

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
            AgentKind::Gemini,
            "IneligibleTierError: This client is no longer supported for Gemini Code \
             Assist for individuals",
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
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

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
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    let path = marker_path(&AgentKind::Qwen, None);
    std::fs::write(&path, "recovery_at: tomorrow morning\nmessage: out of quota\n")
        .expect("write marker");

    assert!(is_rate_limited(&AgentKind::Qwen, None), "fresh marker still holds");
    age_marker(&path, RATE_LIMIT_WINDOW_SECS + 60);
    assert!(
        !is_rate_limited(&AgentKind::Qwen, None),
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
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    for (agent, fixture) in [
        (AgentKind::Codex, "rate-limit-codex"),
        (AgentKind::Qwen, "rate-limit-qwen"),
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
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

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
#[test]
fn a_marker_without_a_hold_field_is_not_permanent() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    let path = marker_path(&AgentKind::Copilot, None);
    std::fs::write(&path, "recovery_at: \nmessage: some old refusal\n").expect("write marker");
    age_marker(&path, RATE_LIMIT_WINDOW_SECS + 60);
    assert!(!is_rate_limited(&AgentKind::Copilot, None));
}

/// Group markers are held by the same three classes as agent markers, so a
/// spent cursor premium pool survives while `auto` is untouched.
#[test]
fn a_group_marker_holds_for_a_person_too() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());
    let _cache = isolate_cache(&temp);

    let cursor = AgentKind::Cursor;
    mark_group_rate_limited(&cursor, None, "premium", "ActionRequiredError: Increase limits for faster responses You're out of usage. \
         Switch to Auto, or ask your admin to increase your limit to continue.");

    age_marker(&group_marker_path(&cursor, None, "premium"), RATE_LIMIT_WINDOW_SECS * 100);
    assert!(is_group_rate_limited(&cursor, None, "premium"));
    assert!(!is_group_rate_limited(&cursor, None, "auto"), "auto keeps serving");
    assert!(!is_rate_limited(&cursor, None), "the agent itself is not written off");

    let holds = active_group_holds(&cursor, None);
    assert_eq!(holds.len(), 1);
    assert_eq!(holds[0].0, "premium");
    assert!(!holds[0].1.needs_human, "cursor premium is Windowed");
    let end = format_hold_end(&cursor, None, &holds[0].1);
    assert!(
        end.contains("aid config clear-limit cursor"),
        "Windowed group hold must still name the clear command, got {end:?}"
    );
    assert!(!end.contains("cooling down"), "got {end:?}");

    assert!(clear_all_rate_limits_for_agent(&cursor, None));
    assert!(!is_group_rate_limited(&cursor, None, "premium"));
    assert!(active_group_holds(&cursor, None).is_empty());
}

/// The live call site, not just the grouping helper. `classify_line` in the
/// cursor adapter has no model in hand, so it marked the agent — which made
/// `is_rate_limited(Cursor, None)` true and took `auto` out with the pool that ran
/// out. The message names the tier; the marking must follow it.
#[test]
fn a_cursor_premium_refusal_with_no_model_in_hand_holds_only_the_premium_pool() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    let cursor = AgentKind::Cursor;
    mark_rate_limited_for_message(&cursor, None, "ActionRequiredError: Increase limits for faster responses You're out of usage. \
         Switch to Auto, or ask your admin to increase your limit to continue.");

    assert!(is_group_rate_limited(&cursor, None, "premium"), "the spent pool is held");
    assert!(!is_group_rate_limited(&cursor, None, "auto"), "auto keeps serving");
    assert!(!is_rate_limited(&cursor, None), "the agent as a whole is not written off");
    assert!(
        dispatch_blocking_hold(&cursor, None).is_none(),
        "aid run must still dispatch cursor — on auto"
    );
}

/// End to end on the channel it actually arrives on. cursor's premium refusal is
/// on stderr and nowhere else — no error envelope in the stream carries it — so
/// this is the coverage that decides whether `Channel::CliStderr` earns its
/// place in the enumeration. If it ever stops passing, the answer is not to
/// widen the stream channel but to say so.
#[test]
fn cursors_premium_refusal_is_read_off_the_stderr_channel() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    let stderr = "ActionRequiredError: Increase limits for faster responses You're out of usage. \
                  Switch to Auto, or ask your admin to increase your limit to continue.";
    let refusal = refusal_on_channel(
        stderr,
        AgentKind::Cursor,
        crate::quota_channel::Channel::CliStderr,
    )
    .expect("cursor's premium refusal must be readable on stderr");
    mark_rate_limited_for_message(&AgentKind::Cursor, None, &refusal);

    assert!(is_group_rate_limited(&AgentKind::Cursor, None, "premium"));
    assert!(!is_group_rate_limited(&AgentKind::Cursor, None, "auto"));
    assert!(!is_rate_limited(&AgentKind::Cursor, None));
}

/// The complement: a cursor refusal that names no tier is still an agent-level
/// fact and must not be quietly narrowed to one group.
#[test]
fn a_cursor_refusal_naming_no_tier_still_marks_the_agent() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    let cursor = AgentKind::Cursor;
    mark_rate_limited_for_message(&cursor, None, "Quota exceeded for this workspace");

    assert!(is_rate_limited(&cursor, None));
    assert!(!is_group_rate_limited(&cursor, None, "premium"));
}

/// `aid run` used to divert only when a recovery time was present. A Windowed
/// 402 states none, so dispatch must still stop and name the dated-snapshot way
/// out rather than inventing a clock or saying "cooling down".
#[test]
fn a_windowed_hold_blocks_dispatch_and_names_the_way_out() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    mark_rate_limited(&AgentKind::Grok, None, "API error (status 402 Payment Required): Grok Build usage balance exhausted");
    let hold = dispatch_blocking_hold(&AgentKind::Grok, None).expect("a Windowed hold must block");
    assert_eq!(
        hold,
        "until a dated grok snapshot with headroom (or `aid config clear-limit grok`)"
    );
    assert!(!hold.contains("cooling down"));

    assert!(clear_rate_limit(&AgentKind::Grok, None));
    assert!(dispatch_blocking_hold(&AgentKind::Grok, None).is_none());
}

/// The other direction of the same gate: a stated time still blocks while it is
/// in the future, and stops blocking once it has passed. oz's live marker is
/// fourteen hours stale and kept diverting runs off a route that was back.
#[test]
fn a_stated_time_blocks_dispatch_only_until_it_passes() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    let content = with_relative_recovery_time(&read_fixture("rate-limit-codex"), Duration::days(1));
    let stated = content
        .lines()
        .find_map(|line| line.strip_prefix("recovery_at: "))
        .map(str::to_string)
        .expect("fixture must state a recovery time");
    std::fs::write(marker_path(&AgentKind::Codex, None), content)
        .expect("write marker");
    assert_eq!(
        dispatch_blocking_hold(&AgentKind::Codex, None),
        Some(format!("until {stated}")),
        "the provider's own phrasing of the time is quoted back"
    );

    let content = with_relative_recovery_time(&read_fixture("rate-limit-oz"), Duration::minutes(-1));
    std::fs::write(marker_path(&AgentKind::Oz, None), content).expect("write marker");
    assert!(
        dispatch_blocking_hold(&AgentKind::Oz, None).is_none(),
        "a reset time in the past must not divert a run"
    );
}

/// A bounded cooldown is not a dispatch gate. Moving the caller off the agent
/// they asked for costs more than the few minutes left on a transient 429, and
/// gating on it was never the previous behaviour either.
#[test]
fn a_transient_cooldown_does_not_divert_dispatch() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    mark_rate_limited(&AgentKind::Claude, None, "HTTP 429 Too Many Requests");
    assert!(is_rate_limited(&AgentKind::Claude, None), "still cooling down");
    assert!(dispatch_blocking_hold(&AgentKind::Claude, None).is_none());
}

/// Markers already on disk when this version lands carry no `hold:` line. The
/// two human-ended ones state no reset time either, so the cooldown would
/// release them within five minutes and hand work back to a spent allowance.
/// Their stored refusal text is the same evidence write-time classification
/// uses, so it is re-read rather than the file rewritten.
#[test]
fn a_legacy_marker_is_reclassified_from_the_refusal_it_stored() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());
    let _cache = isolate_cache(&temp);

    for (agent, fixture, human) in [
        // "recovery_at: " empty, message a mid-token JSON fragment that still
        // carries "You have exceeded your monthly quota".
        (AgentKind::Copilot, "rate-limit-copilot", true),
        // "recovery_at: " empty, refusal on a later line of a multi-line message.
        (AgentKind::Grok, "rate-limit-grok", false),
    ] {
        let path = marker_path(&agent, None);
        std::fs::write(&path, read_fixture(fixture)).expect("write marker");
        age_marker(&path, RATE_LIMIT_WINDOW_SECS * 100);

        assert!(
            is_rate_limited(&agent, None),
            "{fixture} must not expire on a timer"
        );
        let info = get_rate_limit_info(&agent, None).expect("marker present");
        assert_eq!(
            info.needs_human, human,
            "{fixture} must report which kind of hold it is under"
        );
        assert!(dispatch_blocking_hold(&agent, None).is_some(), "{fixture} must divert dispatch");
        if !human {
            assert!(
                !format_hold_end(&agent, None, &info).contains("cooling down"),
                "{fixture} Windowed must not print cooling down"
            );
        }
    }
}

/// The same rule must not turn every legacy marker permanent. A stored message
/// matching no signature is still transient and still expires — that is the
/// blackhole this whole change exists to avoid.
#[test]
fn a_legacy_marker_with_an_unrecognised_refusal_still_expires() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    let path = marker_path(&AgentKind::Claude, None);
    std::fs::write(&path, "recovery_at: \nmessage: 429 Too Many Requests\n")
        .expect("write marker");
    age_marker(&path, RATE_LIMIT_WINDOW_SECS + 60);

    assert!(!is_rate_limited(&AgentKind::Claude, None));
    assert!(!get_rate_limit_info(&AgentKind::Claude, None).expect("marker present").needs_human);
}

/// A marker records what *one* provider said, so a needle another provider owns
/// is not evidence about this one. `~/.aid/rate-limit-claude` was written on
/// 2026-08-07 from an agent's own message quoting this crate's signature table,
/// and the stored text contained `"insufficient balance"` — opencode's refusal,
/// which held claude open until someone cleared it.
///
/// The scoping is what does the work here, not the wording: the identical text
/// in opencode's own marker is still a human-ended hold, as the second half
/// asserts. The write side can no longer produce such a marker at all
/// (`quota_channel`); this covers the ones already on disk.
#[test]
fn a_stored_refusal_only_speaks_for_the_agent_whose_marker_it_is() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());

    let stored = "recovery_at: \nmessage: QuotaSignature { needle: \"insufficient balance\", \
                  recovery: QuotaRecovery::NeedsHuman }\n";

    let claude = marker_path(&AgentKind::Claude, None);
    std::fs::write(&claude, stored).expect("write marker");
    age_marker(&claude, RATE_LIMIT_WINDOW_SECS + 60);
    assert!(
        !is_rate_limited(&AgentKind::Claude, None),
        "another provider's needle must not hold claude open until someone clears it"
    );

    let opencode = marker_path(&AgentKind::OpenCode, None);
    std::fs::write(&opencode, stored).expect("write marker");
    age_marker(&opencode, RATE_LIMIT_WINDOW_SECS + 60);
    assert!(
        is_rate_limited(&AgentKind::OpenCode, None),
        "opencode's own refusal must still hold, or this became a denylist"
    );
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

/// Shift a fixture's stated time relative to now while preserving its provider
/// formatting. The message body remains the captured evidence under test.
fn with_relative_recovery_time(content: &str, delta: Duration) -> String {
    let stated = content
        .lines()
        .find_map(|line| line.strip_prefix("recovery_at: "))
        .filter(|value| !value.is_empty())
        .expect("fixture must state a recovery time");
    let template_tokens: Vec<_> = stated.split_whitespace().collect();
    let at = Local::now().naive_local() + delta;
    let day_token = template_tokens[1];
    let day = if day_token.starts_with('0') {
        format!("{:02}", at.day())
    } else {
        at.day().to_string()
    };
    let suffix = if day_token.ends_with("st,") || day_token.ends_with("nd,")
        || day_token.ends_with("rd,") || day_token.ends_with("th,")
    {
        ordinal_suffix(at.day()).to_string()
    } else {
        String::new()
    };
    let hour = if template_tokens[3].starts_with('0') {
        format!("{:02}", at.hour12().1)
    } else {
        at.hour12().1.to_string()
    };
    let replacement = format!(
        "{} {}{}, {} {}:{:02} {}",
        at.format("%b"), day, suffix, at.year(), hour, at.minute(), at.format("%p")
    );
    content.replacen(
        &format!("recovery_at: {stated}"),
        &format!("recovery_at: {replacement}"),
        1,
    )
}

fn ordinal_suffix(day: u32) -> &'static str {
    match day % 100 {
        11..=13 => "th",
        _ => match day % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        },
    }
}
