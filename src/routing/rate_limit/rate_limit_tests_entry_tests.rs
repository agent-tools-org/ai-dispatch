// Production entry point regression tests split to keep the parent within its size limit.
// Deps: parent test fixtures and production APIs.

use super::*;

#[test]
fn generic_quota_signals_apply_on_non_agent_channels() {
    assert!(
        crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .any(|agent| {
                crate::routing::rate_limit::is_rate_limit_error_for_agent_with_evidence(
                    "rate limit exceeded",
                    agent,
                    QuotaEvidence::NonAgentChannel,
                )
            })
    );
    assert!(
        crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .any(|agent| {
                crate::routing::rate_limit::is_rate_limit_error_for_agent_with_evidence(
                    "HTTP 429 Too Many Requests",
                    agent,
                    QuotaEvidence::NonAgentChannel,
                )
            })
    );
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
    assert!(
        !crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .any(|agent| {
                crate::routing::rate_limit::is_rate_limit_error_for_agent_with_evidence(
                    "We must respect the rate limit on the Base sequencer feed",
                    agent,
                    QuotaEvidence::AgentProse,
                )
            })
    );
}

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
        refusal_on_channel("429 rate limit exceeded", AgentKind::Cursor, Channel::CliStream),
        None
    );
}

#[test]
fn stderr_carries_droids_402_body() {
    let line = concat!(
        r#"402 {"detail":"You've reached your 5-hour standard usage limit "#,
        r#"(resets in 1h 48min).","status":402}"#,
    );
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
        &format!(
            "You have hit your usage limit. try again at {}.",
            test_future_recovery_time()
        ),
    );
    assert!(is_rate_limited(&AgentKind::Codex, None));

    let _ = std::fs::remove_file(marker_path(&AgentKind::Codex, None));
    assert!(!is_rate_limited(&AgentKind::Codex, None));
}
