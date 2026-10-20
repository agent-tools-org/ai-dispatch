// Additional regression coverage for hold_tests.rs.
// Deps: parent test module fixtures.

use super::*;

#[test]
fn a_marker_without_a_hold_field_is_not_permanent() {
    let temp = isolated();
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

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
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());
    let _cache = isolate_cache(&temp);
    write_probe_held(&temp.path().join("aidbar"), "cursor");

    let cursor = AgentKind::Cursor;
    mark_group_rate_limited(
        &cursor,
        None,
        "premium",
        "ActionRequiredError: Increase limits for faster responses You're out of usage. \
         Switch to Auto, or ask your admin to increase your limit to continue.",
    );

    age_marker(
        &group_marker_path(&cursor, None, "premium"),
        RATE_LIMIT_WINDOW_SECS * 100,
    );
    assert!(is_group_rate_limited(&cursor, None, "premium"));
    assert!(
        !is_group_rate_limited(&cursor, None, "auto"),
        "auto keeps serving"
    );
    assert!(
        !is_rate_limited(&cursor, None),
        "the agent itself is not written off"
    );

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
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

    let cursor = AgentKind::Cursor;
    mark_rate_limited_for_message(
        &cursor,
        None,
        "ActionRequiredError: Increase limits for faster responses You're out of usage. \
         Switch to Auto, or ask your admin to increase your limit to continue.",
    );

    assert!(
        is_group_rate_limited(&cursor, None, "premium"),
        "the spent pool is held"
    );
    assert!(
        !is_group_rate_limited(&cursor, None, "auto"),
        "auto keeps serving"
    );
    assert!(
        !is_rate_limited(&cursor, None),
        "the agent as a whole is not written off"
    );
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
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

    let stderr = "ActionRequiredError: Increase limits for faster responses You're out of usage. \
                  Switch to Auto, or ask your admin to increase your limit to continue.";
    let refusal = refusal_on_channel(
        stderr,
        AgentKind::Cursor,
        crate::routing::quota::channel::Channel::CliStderr,
    )
    .expect("cursor's premium refusal must be readable on stderr");
    mark_rate_limited_for_message(&AgentKind::Cursor, None, &refusal);

    assert!(is_group_rate_limited(&AgentKind::Cursor, None, "premium"));
    assert!(!is_group_rate_limited(&AgentKind::Cursor, None, "auto"));
    assert!(!is_rate_limited(&AgentKind::Cursor, None));
}

/// oz's logged-out refusal is CLI stderr, not an assistant envelope. Matching
/// it here is what holds the route; widening to model prose would not.
#[test]
fn oz_invalid_credentials_are_read_off_the_stderr_channel() {
    let temp = isolated();
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

    let stderr = "Error: Your credentials are invalid. Please log in again with `oz login`.";
    let refusal = refusal_on_channel(
        stderr,
        AgentKind::Oz,
        crate::routing::quota::channel::Channel::CliStderr,
    )
    .expect("oz credentials refusal must be readable on stderr");
    mark_rate_limited_for_message(&AgentKind::Oz, None, &refusal);

    let info = get_rate_limit_info(&AgentKind::Oz, None).expect("marker written");
    assert!(info.needs_human, "logged-out oz must wait for a person");
    assert!(is_rate_limited(&AgentKind::Oz, None));
    let end = format_hold_end(&AgentKind::Oz, None, &info);
    assert!(
        end.starts_with("needs human: Error: Your credentials are invalid."),
        "got {end:?}"
    );
    assert!(
        end.contains("fix, then `aid config clear-limit oz`"),
        "got {end:?}"
    );
}

/// The complement: a cursor refusal that names no tier is still an agent-level
/// fact and must not be quietly narrowed to one group.
#[test]
fn a_cursor_refusal_naming_no_tier_still_marks_the_agent() {
    let temp = isolated();
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

    let cursor = AgentKind::Cursor;
    mark_rate_limited_for_message(&cursor, None, "Quota exceeded for this workspace");

    assert!(is_rate_limited(&cursor, None));
    assert!(!is_group_rate_limited(&cursor, None, "premium"));
}

/// The live call site with no model in hand. The 402 names the standard
/// pool; Core must stay dispatchable.
#[test]
fn a_droid_standard_402_with_no_model_holds_only_the_standard_pool() {
    let temp = isolated();
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

    let body = r#"{"type":"error","source":"agent_loop","message":"402 {\"detail\":\"You've reached your weekly standard usage limit (resets in 2 days).\nSwitch to Droid Core or enable Extra Usage to continue.\",\"status\":402,\"title\":\"Payment Required\",\"displayToUser\":true}"}"#;
    mark_rate_limited_for_message(&AgentKind::Droid, None, body);

    assert!(is_group_rate_limited(&AgentKind::Droid, None, "standard"));
    assert!(!is_group_rate_limited(&AgentKind::Droid, None, "core"));
    assert!(!is_rate_limited(&AgentKind::Droid, None));
    assert!(
        dispatch_blocking_hold(&AgentKind::Droid, None).is_none(),
        "aid run must still dispatch droid — on Core"
    );
}

#[test]
fn a_droid_core_402_with_no_model_holds_only_the_core_pool() {
    let temp = isolated();
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

    let body = r#"402 {"detail":"You've reached your weekly Droid Core usage limit (resets in 2 days).\nReload Extra Usage credits or wait for your limits to reset.","status":402,"title":"Payment Required","displayToUser":true}"#;
    mark_rate_limited_for_message(&AgentKind::Droid, None, body);

    assert!(is_group_rate_limited(&AgentKind::Droid, None, "core"));
    assert!(!is_group_rate_limited(&AgentKind::Droid, None, "standard"));
    assert!(!is_rate_limited(&AgentKind::Droid, None));
    assert!(
        dispatch_blocking_hold(&AgentKind::Droid, None).is_none(),
        "aid run must still dispatch droid — on standard"
    );
}

#[test]
fn a_droid_refusal_naming_no_tier_still_marks_the_agent() {
    let temp = isolated();
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

    mark_rate_limited_for_message(
        &AgentKind::Droid,
        None,
        "402 payment required: reload your tokens",
    );

    assert!(is_rate_limited(&AgentKind::Droid, None));
    assert!(!is_group_rate_limited(&AgentKind::Droid, None, "standard"));
    assert!(!is_group_rate_limited(&AgentKind::Droid, None, "core"));
}

/// Watcher, lifecycle, and stream-completion pass `requested_model`. The
/// refusal names the spent tier; a dispatched opus must not change that.
#[test]
fn a_droid_standard_402_with_a_model_in_hand_holds_only_the_standard_pool() {
    let temp = isolated();
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

    let body = r#"{"type":"error","source":"agent_loop","message":"402 {\"detail\":\"You've reached your weekly standard usage limit (resets in 2 days).\nSwitch to Droid Core or enable Extra Usage to continue.\",\"status\":402,\"title\":\"Payment Required\",\"displayToUser\":true}"}"#;
    mark_rate_limited_for_model(&AgentKind::Droid, None, Some("claude-opus-5"), body);

    assert!(is_group_rate_limited(&AgentKind::Droid, None, "standard"));
    assert!(!is_group_rate_limited(&AgentKind::Droid, None, "core"));
    assert!(!is_rate_limited(&AgentKind::Droid, None));
    assert!(
        dispatch_blocking_hold(&AgentKind::Droid, None).is_none(),
        "aid run must still dispatch droid — on Core"
    );
}

/// Same write site, Core model in hand: the refusal still names standard.
#[test]
fn a_droid_standard_402_with_a_core_model_in_hand_still_holds_only_standard() {
    let temp = isolated();
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

    let body = r#"{"type":"error","source":"agent_loop","message":"402 {\"detail\":\"You've reached your weekly standard usage limit (resets in 2 days).\nSwitch to Droid Core or enable Extra Usage to continue.\",\"status\":402,\"title\":\"Payment Required\",\"displayToUser\":true}"}"#;
    mark_rate_limited_for_model(&AgentKind::Droid, None, Some("glm-5.2"), body);

    assert!(is_group_rate_limited(&AgentKind::Droid, None, "standard"));
    assert!(!is_group_rate_limited(&AgentKind::Droid, None, "core"));
    assert!(!is_rate_limited(&AgentKind::Droid, None));
}

/// The live with-model path. `reload your tokens` names no tier and is
/// NeedsHuman — a purchase ends it. Marking only `standard` would keep
/// dispatching Core against an account that cannot serve.
#[test]
fn a_droid_reload_tokens_402_with_a_model_in_hand_marks_the_agent() {
    let temp = isolated();
    let _guard = crate::support::paths::AidHomeGuard::set(temp.path());

    mark_rate_limited_for_model(
        &AgentKind::Droid,
        None,
        Some("claude-opus-5"),
        "402 payment required: reload your tokens",
    );

    assert!(is_rate_limited(&AgentKind::Droid, None));
    assert!(!is_group_rate_limited(&AgentKind::Droid, None, "standard"));
    assert!(!is_group_rate_limited(&AgentKind::Droid, None, "core"));
    assert!(dispatch_blocking_hold(&AgentKind::Droid, None).is_some());
    assert!(
        dispatch_blocking_hold_for_model(&AgentKind::Droid, None, Some("glm-5.2")).is_none(),
        "Core group is not held; the agent-wide marker is what stops dispatch"
    );
}

/// `aid run` used to divert only when a recovery time was present. A Windowed
/// 402 states none, so dispatch must still stop and name the dated-snapshot way
/// out rather than inventing a clock or saying "cooling down". A probe is
/// written so the route keeps its Windowed class — see the probe-less case below.

#[path = "hold_tests_more_2.rs"]
mod more;
