// Existing refusal and marker regression tests.
// Deps: parent rate-limit facade and isolated aid home.

use super::*;
use crate::support::paths;

/// The exact string codex produced on 2026-08-05. If this fails to parse,
/// `is_rate_limited` falls back to a 300-second mtime window and a six-day
/// outage reads as available again after five minutes.
#[test]
fn codex_recovery_timestamp_parses() {
    let stated = test_future_recovery_time();
    let message = format!(
        "You've hit your usage limit. Visit https://chatgpt.com/codex/settings/usage \
         to purchase more credits or try again at {stated}."
    );
    let extracted = parse_recovery_time(&message).expect("recovery phrase must be extracted");
    assert_eq!(extracted, stated);
    let parsed = parse_recovery_datetime(&extracted).expect("recovery timestamp must parse");
    assert!(
        parsed > Local::now().naive_local(),
        "parsed {parsed} must be in the future"
    );
}

#[test]
fn format_recovery_output_round_trips_through_parse_recovery_datetime() {
    // format_recovery writes "%b %d, %Y %I:%M %p" — e.g. "Aug 18, 2026 02:41 PM".
    // A dated hold whose marker aid wrote itself must read back as held, or
    // the dispatch picker treats it as a long-expired transient (t-44b30780).
    for minutes_from_now in [1, 232, 1440] {
        let at = Local::now().naive_local() + chrono::Duration::minutes(minutes_from_now);
        let written = format_recovery(at);
        let parsed = parse_recovery_datetime(&written)
            .unwrap_or_else(|| panic!("format_recovery output must parse back: {written:?}"));
        // Minute precision: the format drops seconds, so compare to the minute.
        let expected = at.date().and_hms_opt(at.hour(), at.minute(), 0).unwrap();
        assert_eq!(parsed, expected, "round trip of {written:?} must be exact");
    }
}

#[test]
fn test_is_rate_limit_error() {
    assert!(is_rate_limit_error(
        "You have hit your usage limit. try again at Mar 19th, 2026 2:27 PM."
    ));
    assert!(is_rate_limit_error(
        "Error: Quota limit reached."
    ));
    assert!(is_rate_limit_error(
        "APIError: Insufficient balance. Manage your billing here"
    ));
    assert!(is_rate_limit_error(
        "402 payment required: reload your tokens"
    ));
    assert!(is_rate_limit_error(
        "IneligibleTierError: This client is no longer supported for Gemini Code Assist for individuals; migrate to Antigravity"
    ));
    assert!(!is_rate_limit_error("network timeout"));
    assert!(!is_rate_limit_error("connection refused"));
    assert!(!is_rate_limit_error("payment required"));
    assert!(!is_rate_limit_error(
        "503 No accounts with a plan supporting gpt-4.1-nano"
    ));
    assert!(!is_rate_limit_error(
        "tokens: 8714294 in + 27373 out = 8741667 (8442752 cached)"
    ));
    assert!(!is_rate_limit_error("invoice 1402 created"));
}

#[test]
fn prose_mentions_rate_limit_is_not_quota_failure() {
    assert!(!is_rate_limit_error(
        "rate_limit_kind now returns AgentKind::Custom for custom agents"
    ));
    assert!(!is_rate_limit_error(
        "The RPC provider throttles us; we saw a 429 and burned Alchemy credits"
    ));
    assert!(!is_rate_limit_error(
        "We must respect the rate limit on the Base sequencer feed"
    ));
    assert!(!is_rate_limit_error(
        "The parser handles nested arrays correctly"
    ));
}

#[test]
fn generic_quota_signals_apply_on_non_agent_channels() {
    assert!(is_rate_limit_error_with_evidence(
        "rate limit exceeded",
        QuotaEvidence::NonAgentChannel
    ));
    assert!(is_rate_limit_error_with_evidence(
        "HTTP 429 Too Many Requests",
        QuotaEvidence::NonAgentChannel
    ));
    assert!(is_rate_limit_error_for_agent_with_evidence(
        "rate limit exceeded",
        &AgentKind::Claude,
        QuotaEvidence::NonAgentChannel
    ));
    assert!(is_rate_limit_error_for_agent_with_evidence(
        "429 Too Many Requests",
        &AgentKind::Grok,
        QuotaEvidence::NonAgentChannel
    ));
    assert!(!is_rate_limit_error_with_evidence(
        "We must respect the rate limit on the Base sequencer feed",
        QuotaEvidence::AgentProse
    ));
}

use crate::routing::quota::channel::Channel;

#[test]
fn a_generic_status_token_is_read_inside_an_envelope_the_cli_opened() {
    assert_eq!(
        refusal_on_channel(
            r#"{"type":"error","error":{"message":"429 rate limit exceeded"}}"#,
            AgentKind::Cursor,
            Channel::CliStream,
        ),
        Some("429 rate limit exceeded".to_string())
    );
}

#[test]
fn a_generic_status_token_outside_an_envelope_is_not_a_refusal() {
    // Same token, same channel, no envelope around it: this is the shape a
    // PTY renders the model's own answer in.
    assert_eq!(
        refusal_on_channel(
            "429 rate limit exceeded",
            AgentKind::Cursor,
            Channel::CliStream
        ),
        None
    );
}

#[test]
fn stderr_carries_droids_402_body() {
    let line = r#"402 {"detail":"You've reached your 5-hour standard usage limit (resets in 1h 48min).","status":402}"#;
    assert!(
        refusal_on_channel(line, AgentKind::Droid, Channel::CliStderr)
            .is_some_and(|refusal| refusal.contains("standard usage limit")),
        "the refusal droid actually wrote on 2026-08-07 must stay detectable"
    );
}

#[test]
fn an_agents_own_words_about_a_provider_are_not_a_refusal() {
    for line in [
        "The RPC provider throttles us; we saw a 429 during the run.",
        "completed: grep clear_rate_limit_if_stale|marker_path",
        "YOLO mode is enabled",
    ] {
        assert_eq!(
            refusal_on_channel(line, AgentKind::Cursor, Channel::CliStream),
            None,
            "{line:?} is the model or aid talking"
        );
    }
}

#[test]
fn an_init_event_mentioning_rate_limits_is_not_a_refusal() {
    assert_eq!(
        refusal_on_channel(
            r#"{"type":"system","subtype":"init","message":"rate limit enabled"}"#,
            AgentKind::Codex,
            Channel::CliStream,
        ),
        None
    );
}

#[test]
fn codexs_usage_limit_envelope_yields_its_own_sentence() {
    let message = "You've hit your usage limit. Visit https://chatgpt.com/codex/settings/usage \
                     to purchase more credits or try again at Aug 11th, 2026 2:23 PM.";
    assert_eq!(
        refusal_on_channel(
            &format!(r#"{{"type":"error","message":"{message}"}}"#),
            AgentKind::Codex,
            Channel::CliStream,
        ),
        Some(message.to_string()),
    );
}

/// A signature belongs to one provider. cursor's needle in copilot's stream
/// is a report about cursor, not copilot refusing.
#[test]
fn a_refusal_is_only_read_for_the_agent_that_owns_the_signature() {
    let envelope = r#"{"type":"error","message":"You're out of usage. Switch to Auto."}"#;
    assert!(refusal_on_channel(envelope, AgentKind::Cursor, Channel::CliStream).is_some());
    assert_eq!(
        refusal_on_channel(envelope, AgentKind::Copilot, Channel::CliStream),
        None
    );
}

#[test]
fn test_mark_and_check_rate_limited() {
    let temp_dir = std::env::temp_dir().join("aid-rate-limit-test");
    let _guard = paths::AidHomeGuard::set(&temp_dir);
    std::fs::create_dir_all(paths::aid_dir()).ok();

    mark_rate_limited(
        &AgentKind::Codex,
        None,
        &format!("try again at {}.", test_future_recovery_time()),
    );
    assert!(is_rate_limited(&AgentKind::Codex, None));

    let _ = std::fs::remove_file(marker_path(&AgentKind::Codex, None));
    assert!(!is_rate_limited(&AgentKind::Codex, None));
}
