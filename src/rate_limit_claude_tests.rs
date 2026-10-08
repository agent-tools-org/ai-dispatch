// Regression tests for Claude reset suffixes and CLI-only refusal evidence.
// Deps: super, quota_channel, agent::stream_completion, rate_limit.

use super::*;
use crate::agent::stream_completion::record_quota_exhaustion;
use crate::quota_channel::Channel;
use crate::rate_limit::{self, get_rate_limit_info, mark_rate_limited, refusal_on_channel};
use crate::types::AgentKind;

#[test]
fn unrelated_429_with_unix_trace_does_not_create_a_dated_hold() {
    let message = "HTTP 429 downloading an artifact; trace|1893456000";
    assert_eq!(parse_recovery_time(message), None);
    let temp = tempfile::tempdir().expect("temp dir");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    let event = serde_json::json!({"type": "error", "message": message}).to_string();
    let refusal = refusal_on_channel(&event, AgentKind::Cursor, Channel::CliStream)
        .expect("429 in error envelope");
    mark_rate_limited(&AgentKind::Cursor, None, &refusal);
    let info = get_rate_limit_info(&AgentKind::Cursor, None).expect("marker");
    assert_eq!(info.recovery_at, None, "{info:?}");
    assert!(!rate_limit::is_rate_limited(&AgentKind::Cursor, None));
}

#[test]
fn unwrapped_claude_prose_cannot_create_a_hold() {
    let temp = tempfile::tempdir().expect("temp dir");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
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
        assert!(!record_quota_exhaustion(line, AgentKind::Claude, None, None).recorded());
    }
    assert_eq!(
        refusal_on_channel("You've hit your limit", AgentKind::Claude, Channel::CliStderr),
        None
    );
    assert!(get_rate_limit_info(&AgentKind::Claude, None).is_none());
}

#[test]
fn rejected_claude_event_uses_its_nearby_unix_reset() {
    let temp = tempfile::tempdir().expect("temp dir");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
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
    let temp = tempfile::tempdir().expect("temp dir");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
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
    let found = crate::auth_marker::auth_failure_line(line, AgentKind::Claude, Channel::CliStream);
    assert!(found.is_some_and(|text| text.contains("/login")));
    assert_eq!(refusal_on_channel(line, AgentKind::Claude, Channel::CliStream), None);
}
