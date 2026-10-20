// Additional regression coverage for hold_tests.rs.
// Deps: parent test module fixtures.

use super::*;

#[test]
fn a_windowed_hold_blocks_dispatch_and_names_the_way_out() {
    let temp = isolated();
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());
    let _cache = isolate_cache(&temp);
    write_probe_held(&temp.path().join("aidbar"), "grok");

    mark_rate_limited(
        &AgentKind::Grok,
        None,
        "API error (status 402 Payment Required): Grok Build usage balance exhausted",
    );
    let hold = dispatch_blocking_hold(&AgentKind::Grok, None).expect("a Windowed hold must block");
    assert_eq!(
        hold,
        "until a dated grok snapshot with headroom (or `aid config clear-limit grok`)"
    );
    assert!(!hold.contains("cooling down"));

    assert!(clear_rate_limit(&AgentKind::Grok, None));
    assert!(dispatch_blocking_hold(&AgentKind::Grok, None).is_none());
}

/// A probe-less agent's Windowed refusal must state the human-cleared end, not
/// promise a dated snapshot that can never arrive. grok's 402 is Windowed by
/// signature (aidbar is assumed to map the billing period to a dated resets_at),
/// but when no probe exists the recovery condition is unobservable, so the hold
/// is human-cleared and `aid agent quota` / dispatch must say so plainly.
#[test]
fn a_probeless_windowed_refusal_states_a_human_clear_not_a_missing_snapshot() {
    let temp = isolated();
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());
    let _cache = isolate_cache(&temp);

    mark_rate_limited(
        &AgentKind::Grok,
        None,
        "API error (status 402 Payment Required): Grok Build usage balance exhausted",
    );

    let hold = dispatch_blocking_hold(&AgentKind::Grok, None)
        .expect("the refusal must still block dispatch");
    assert!(
        !hold.contains("dated grok snapshot"),
        "a probe-less route must not promise a snapshot; got {hold:?}"
    );
    assert!(
        hold.contains("aid config clear-limit grok"),
        "the stated end must name the human-clear command; got {hold:?}"
    );

    let info = get_rate_limit_info(&AgentKind::Grok, None).expect("marker written");
    assert!(
        info.needs_human,
        "a probe-less Windowed refusal is human-cleared, not windowed: {info:?}"
    );
    let end = format_hold_end(&AgentKind::Grok, None, &info);
    assert!(
        !end.contains("dated grok snapshot"),
        "quota display must not promise a snapshot for a probe-less route: {end:?}"
    );
    assert!(
        end.contains("aid config clear-limit grok"),
        "quota display must name the human-clear command: {end:?}"
    );
}

/// The other direction of the same gate: a stated time still blocks while it is
/// in the future, and stops blocking once it has passed. oz's live marker is
/// fourteen hours stale and kept diverting runs off a route that was back.
#[test]
fn a_stated_time_blocks_dispatch_only_until_it_passes() {
    let temp = isolated();
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

    let content = with_relative_recovery_time(&read_fixture("rate-limit-codex"), Duration::days(1));
    let stated = content
        .lines()
        .find_map(|line| line.strip_prefix("recovery_at: "))
        .map(str::to_string)
        .expect("fixture must state a recovery time");
    std::fs::write(marker_path(&AgentKind::Codex, None), content).expect("write marker");
    assert_eq!(
        dispatch_blocking_hold(&AgentKind::Codex, None),
        Some(format!("until {stated}")),
        "the provider's own phrasing of the time is quoted back"
    );

    let content =
        with_relative_recovery_time(&read_fixture("rate-limit-oz"), Duration::minutes(-1));
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
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

    mark_rate_limited(&AgentKind::Claude, None, "HTTP 429 Too Many Requests");
    assert!(
        !is_rate_limited(&AgentKind::Claude, None),
        "Transient is not Held, so scoring no longer applies -10"
    );
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
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());
    let _cache = isolate_cache(&temp);
    write_probe_held(&temp.path().join("aidbar"), "grok");

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
        assert!(
            dispatch_blocking_hold(&agent, None).is_some(),
            "{fixture} must divert dispatch"
        );
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
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

    let path = marker_path(&AgentKind::Claude, None);
    std::fs::write(&path, "recovery_at: \nmessage: 429 Too Many Requests\n").expect("write marker");
    age_marker(&path, RATE_LIMIT_WINDOW_SECS + 60);

    assert!(!is_rate_limited(&AgentKind::Claude, None));
    assert!(
        !get_rate_limit_info(&AgentKind::Claude, None)
            .expect("marker present")
            .needs_human
    );
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
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

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
