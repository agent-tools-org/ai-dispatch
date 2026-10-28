// Regression tests for Claude reset suffixes and CLI-only refusal evidence.
// Deps: super, quota_channel, agent::stream_completion, rate_limit.

use super::*;
use crate::agent::stream_completion::record_quota_exhaustion;
use crate::routing::quota::channel::Channel;
use crate::routing::rate_limit::{self, get_rate_limit_info, mark_rate_limited, refusal_on_channel};
use crate::types::AgentKind;

#[test]
fn droid_default_can_launch_with_only_core_held_refusal_regression() {
    let _temp = crate::support::test_env::TestHome::new();
    let droid = AgentKind::Droid;
    rate_limit::mark_rate_limited_for_message(
        &droid,
        None,
        "You've reached your monthly Droid Core usage limit (resets in 2 days).",
    );
    assert_eq!(
        crate::agent::model_group::model_group(droid, None),
        Some("standard")
    );
    assert!(rate_limit::dispatch_blocking_hold_for_model(&droid, None, None).is_none());
    assert!(rate_limit::dispatch_blocking_hold_for_model(&droid, None, Some("glm-5.2")).is_some());
}

#[test]
fn monthly_droid_core_provider_error_holds_until_reset_refusal_regression() {
    let message = "402 You've reached your monthly Droid Core usage limit (resets in 2 days).";
    assert!(
        crate::routing::rate_limit::signatures::match_quota_signature_for_agent(message, AgentKind::Droid).is_some()
    );
    for channel in [Channel::CliStream, Channel::CliStderr] {
        let _temp = crate::support::test_env::TestHome::new();
        let event = serde_json::json!({"type": "error", "message": message}).to_string();
        if channel == Channel::CliStderr {
            assert_eq!(refusal_on_channel(&event, AgentKind::Droid, channel), None);
            continue;
        }
        let refusal = refusal_on_channel(&event, AgentKind::Droid, channel).expect("refusal");
        rate_limit::mark_rate_limited_for_message(&AgentKind::Droid, None, &refusal);
        let marker = rate_limit::group_marker_path(&AgentKind::Droid, None, "core");
        let body = std::fs::read_to_string(marker).expect("Core marker");
        let reset = body
            .lines()
            .find_map(|line| line.strip_prefix("recovery_at: "))
            .expect("reset");
        let at = rate_limit::parse_recovery_datetime(reset).expect("date");
        assert!((47..=48).contains(&(at - Local::now().naive_local()).num_hours()));
        assert!(rate_limit::is_group_rate_limited(&AgentKind::Droid, None, "core"));
        assert!(!rate_limit::is_group_rate_limited(
            &AgentKind::Droid,
            None,
            "standard"
        ));
        let prose = serde_json::json!({"type": "assistant_message", "content": message}).to_string();
        assert_eq!(
            refusal_on_channel(&prose, AgentKind::Droid, Channel::CliStream),
            None
        );
    }
}

#[test]
fn claude_seven_day_hold_lasts_until_structured_reset_refusal_regression() {
    let _temp = crate::support::test_env::TestHome::new();
    let reset = Utc::now().timestamp().div_euclid(60) * 60 + 7 * 24 * 3600;
    let event = serde_json::json!({
        "type": "rate_limit_event",
        "rate_limit_info": {"status": "rejected", "rateLimitType": "seven_day", "resetsAt": reset}
    })
    .to_string();
    assert!(record_quota_exhaustion(&event, AgentKind::Claude, None, None).recorded());
    let info = get_rate_limit_info(&AgentKind::Claude, None).expect("hold");
    let expected = format_recovery(
        DateTime::from_timestamp(reset, 0)
            .expect("timestamp")
            .with_timezone(&Local)
            .naive_local(),
    );
    assert_eq!(info.recovery_at.as_deref(), Some(expected.as_str()));
    assert_eq!(
        rate_limit::parse_recovery_datetime(&expected),
        Some(
            DateTime::from_timestamp(reset, 0)
                .expect("timestamp")
                .with_timezone(&Local)
                .naive_local()
        )
    );
    assert!(rate_limit::is_rate_limited(&AgentKind::Claude, None));
}

#[test]
fn expired_claude_seven_day_reset_releases_hold_refusal_regression() {
    let _temp = crate::support::test_env::TestHome::new();
    let event = serde_json::json!({
        "type": "rate_limit_event",
        "rate_limit_info": {"status": "rejected", "rateLimitType": "seven_day",
            "resetsAt": Utc::now().timestamp() - 60}
    })
    .to_string();
    assert!(record_quota_exhaustion(&event, AgentKind::Claude, None, None).recorded());
    assert!(!rate_limit::is_rate_limited(&AgentKind::Claude, None));
}

#[test]
fn unrelated_429_with_unix_trace_does_not_create_a_dated_hold() {
    let message = "HTTP 429 downloading an artifact; trace|1893456000";
    assert_eq!(parse_recovery_time(message), None);
    let _temp = crate::support::test_env::TestHome::new();
    let event = serde_json::json!({"type": "error", "message": message}).to_string();
    let refusal = refusal_on_channel(&event, AgentKind::Cursor, Channel::CliStream)
        .expect("429 in error envelope");
    mark_rate_limited(&AgentKind::Cursor, None, &refusal);
    let info = get_rate_limit_info(&AgentKind::Cursor, None).expect("marker");
    assert_eq!(info.recovery_at, None, "{info:?}");
    assert!(!rate_limit::is_rate_limited(&AgentKind::Cursor, None));
}

#[test]
fn unwrapped_claude_prose_can_only_preserve_a_transient_excerpt() {
    let _temp = crate::support::test_env::TestHome::new();
    for line in [
        "Fixture says You've hit your limit",
        "Fixture says You've hit your limit|1893456000",
        "HTTP 429 downloading an artifact; trace|1893456000",
        "You've hit your limit",
    ] {
        assert_eq!(
            refusal_on_channel(line, AgentKind::Claude, Channel::CliStream),
            None,
            "{line}"
        );
        let outcome = record_quota_exhaustion(line, AgentKind::Claude, None, None);
        assert!(!outcome.should_fail());
        assert_eq!(outcome.recorded(), line.contains("You've hit your limit"));
    }
    assert_eq!(
        refusal_on_channel("You've hit your limit", AgentKind::Claude, Channel::CliStderr),
        Some("You've hit your limit".to_string())
    );
    let info = get_rate_limit_info(&AgentKind::Claude, None).expect("transient excerpt");
    assert!(info.recovery_at.is_none());
    assert!(!info.needs_human);
}

#[test]
fn rejected_claude_event_uses_its_nearby_unix_reset() {
    let _temp = crate::support::test_env::TestHome::new();
    let reset = Utc::now().timestamp() + 7200;
    let event = serde_json::json!({
        "type": "rate_limit_event",
        "rate_limit_info": {"status": "rejected", "resetsAt": reset}
    }).to_string();
    assert!(record_quota_exhaustion(&event, AgentKind::Claude, None, None).recorded());
    let info = get_rate_limit_info(&AgentKind::Claude, None).expect("marker");
    let expected = format_recovery(
        DateTime::from_timestamp(reset, 0).expect("timestamp").with_timezone(&Local).naive_local()
    );
    assert_eq!(info.recovery_at.as_deref(), Some(expected.as_str()));
    assert!(rate_limit::is_rate_limited(&AgentKind::Claude, None));
}

#[test]
fn far_future_claude_reset_falls_back_to_signature_window() {
    let _temp = crate::support::test_env::TestHome::new();
    let reset = Utc::now().timestamp() + 30 * 24 * 3600;
    let event = serde_json::json!({
        "type": "rate_limit_event",
        "rate_limit_info": {"status": "rejected", "resetsAt": reset}
    }).to_string();
    assert!(record_quota_exhaustion(&event, AgentKind::Claude, None, None).recorded());
    let info = get_rate_limit_info(&AgentKind::Claude, None).expect("marker");
    let at = rate_limit::parse_recovery_datetime(
        info.recovery_at.as_deref().expect("bounded fallback")
    ).expect("parse fallback");
    let minutes = (at - Local::now().naive_local()).num_minutes();
    assert!((298..=300).contains(&minutes), "expected five-hour fallback, got {minutes}");
}

#[test]
fn plain_claude_stdout_still_sets_the_auth_marker() {
    let line = "Invalid API key · Please run /login";
    let found = crate::routing::auth_marker::auth_failure_line(line, AgentKind::Claude, Channel::CliStream);
    assert!(found.is_some_and(|text| text.contains("/login")));
    assert_eq!(refusal_on_channel(line, AgentKind::Claude, Channel::CliStream), None);
}
