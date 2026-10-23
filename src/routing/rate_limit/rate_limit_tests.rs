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
    assert!(
        crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .any(|agent| {
                crate::routing::rate_limit::is_rate_limit_error_for_agent_with_evidence(
                    "You have hit your usage limit. try again at Mar 19th, 2026 2:27 PM.",
                    agent,
                    crate::routing::rate_limit::QuotaEvidence::AgentProse,
                )
            })
    );
    assert!(
        crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .any(|agent| {
                crate::routing::rate_limit::is_rate_limit_error_for_agent_with_evidence(
                    "Error: Quota limit reached.",
                    agent,
                    crate::routing::rate_limit::QuotaEvidence::AgentProse,
                )
            })
    );
    assert!(
        crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .any(|agent| {
                crate::routing::rate_limit::is_rate_limit_error_for_agent_with_evidence(
                    "APIError: Insufficient balance. Manage your billing here",
                    agent,
                    crate::routing::rate_limit::QuotaEvidence::AgentProse,
                )
            })
    );
    assert!(
        crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .any(|agent| {
                crate::routing::rate_limit::is_rate_limit_error_for_agent_with_evidence(
                    "402 payment required: reload your tokens",
                    agent,
                    crate::routing::rate_limit::QuotaEvidence::AgentProse,
                )
            })
    );
    assert!(
        crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .any(|agent| {
                crate::routing::rate_limit::is_rate_limit_error_for_agent_with_evidence(
                    concat!(
                        "IneligibleTierError: This client is no longer supported for Gemini Code Assist ",
                        "for individuals; migrate to Antigravity"
                    ),
                    agent,
                    crate::routing::rate_limit::QuotaEvidence::AgentProse,
                )
            })
    );
    assert!(
        !crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .any(|agent| {
                crate::routing::rate_limit::is_rate_limit_error_for_agent_with_evidence(
                    "network timeout",
                    agent,
                    crate::routing::rate_limit::QuotaEvidence::AgentProse,
                )
            })
    );
    assert!(
        !crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .any(|agent| {
                crate::routing::rate_limit::is_rate_limit_error_for_agent_with_evidence(
                    "connection refused",
                    agent,
                    crate::routing::rate_limit::QuotaEvidence::AgentProse,
                )
            })
    );
    assert!(
        !crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .any(|agent| {
                crate::routing::rate_limit::is_rate_limit_error_for_agent_with_evidence(
                    "payment required",
                    agent,
                    crate::routing::rate_limit::QuotaEvidence::AgentProse,
                )
            })
    );
    assert!(
        !crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .any(|agent| {
                crate::routing::rate_limit::is_rate_limit_error_for_agent_with_evidence(
                    "503 No accounts with a plan supporting gpt-4.1-nano",
                    agent,
                    crate::routing::rate_limit::QuotaEvidence::AgentProse,
                )
            })
    );
    assert!(
        !crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .any(|agent| {
                crate::routing::rate_limit::is_rate_limit_error_for_agent_with_evidence(
                    "tokens: 8714294 in + 27373 out = 8741667 (8442752 cached)",
                    agent,
                    crate::routing::rate_limit::QuotaEvidence::AgentProse,
                )
            })
    );
    assert!(
        !crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .any(|agent| {
                crate::routing::rate_limit::is_rate_limit_error_for_agent_with_evidence(
                    "invoice 1402 created",
                    agent,
                    crate::routing::rate_limit::QuotaEvidence::AgentProse,
                )
            })
    );
}

#[test]
fn prose_mentions_rate_limit_is_not_quota_failure() {
    assert!(
        !crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .any(|agent| {
                crate::routing::rate_limit::is_rate_limit_error_for_agent_with_evidence(
                    "rate_limit_kind now returns AgentKind::Custom for custom agents",
                    agent,
                    crate::routing::rate_limit::QuotaEvidence::AgentProse,
                )
            })
    );
    assert!(
        !crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .any(|agent| {
                crate::routing::rate_limit::is_rate_limit_error_for_agent_with_evidence(
                    "The RPC provider throttles us; we saw a 429 and burned Alchemy credits",
                    agent,
                    crate::routing::rate_limit::QuotaEvidence::AgentProse,
                )
            })
    );
    assert!(
        !crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .any(|agent| {
                crate::routing::rate_limit::is_rate_limit_error_for_agent_with_evidence(
                    "We must respect the rate limit on the Base sequencer feed",
                    agent,
                    crate::routing::rate_limit::QuotaEvidence::AgentProse,
                )
            })
    );
    assert!(
        !crate::types::AgentKind::ALL
            .iter()
            .chain([&crate::types::AgentKind::Claude])
            .any(|agent| {
                crate::routing::rate_limit::is_rate_limit_error_for_agent_with_evidence(
                    "The parser handles nested arrays correctly",
                    agent,
                    crate::routing::rate_limit::QuotaEvidence::AgentProse,
                )
            })
    );
}

use crate::routing::quota::channel::Channel;

#[path = "rate_limit_tests_entry_tests.rs"]
mod production_entries;
