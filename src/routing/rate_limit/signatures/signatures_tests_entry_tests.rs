// Production entry point regression tests split to keep the parent within its size limit.
// Deps: parent test fixtures and production APIs.

use super::*;

#[test]
fn grok_usage_balance_exhausted_is_windowed() {
    let msg = "API error (status 402 Payment Required): Grok Build usage balance exhausted";
    let (agent, recovery) = crate::types::AgentKind::ALL
        .iter().chain([&crate::types::AgentKind::Claude])
        .find_map(|agent| {
            crate::routing::rate_limit::signatures::match_quota_signature_for_agent(msg, *agent)
                .map(|recovery| (*agent, recovery))
        })
        .expect("grok 402 must match");
    assert_eq!(agent, AgentKind::Grok);
    assert_eq!(recovery, QuotaRecovery::Windowed);
}

#[test]
fn cursor_out_of_usage_is_windowed() {
    let msg = "ActionRequiredError: Increase limits for faster responses You're out of usage. \
               Switch to Auto, or ask your admin to increase your limit to continue.";
    let (agent, recovery) = crate::types::AgentKind::ALL
        .iter().chain([&crate::types::AgentKind::Claude])
        .find_map(|agent| {
            crate::routing::rate_limit::signatures::match_quota_signature_for_agent(msg, *agent)
                .map(|recovery| (*agent, recovery))
        })
        .expect("cursor premium must match");
    assert_eq!(agent, AgentKind::Cursor);
    assert_eq!(recovery, QuotaRecovery::Windowed);
}

#[test]
fn claude_limit_wording_has_a_clock_ended_signature() {
    for message in [
        "You've hit your limit",
        "Claude AI usage limit reached|1893456000",
    ] {
        assert_eq!(
            match_quota_signature_for_agent(message, AgentKind::Claude),
            Some(QuotaRecovery::After(300)),
            "{message}"
        );
    }
}

#[test]
fn claude_allowed_event_with_rejected_overage_does_not_hold() {
    // Captured Claude stream-json line; overageStatus is not the limit status.
    let allowed = concat!(
        r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt""#,
        r#":1790750400,"rateLimitType":"five_hour","overageStatus":"rejected","overage"#,
        r#"DisabledReason":"org_level_disabled_until","isUsingOverage":false,"unifiedW"#,
        r#"indows":{"five_hour":{"utilization":0.76,"resetsAt":1790750400},"seven_day""#,
        r#":{"utilization":0.7,"resetsAt":1790748000}}},"uuid":"0433b702-fa48-4e68-ae3"#,
        r#"2-414ffeee1b33","session_id":"3eaf9e7b-31e3-482c-bb96-66e14f3bf04a"}"#,
    );
    let temp = tempfile::tempdir().expect("temp dir");
    let _home = crate::support::paths::AidHomeGuard::set(temp.path());
    assert_eq!(
        crate::routing::rate_limit::refusal_on_channel(
            allowed,
            AgentKind::Claude,
            crate::routing::quota::channel::Channel::CliStream,
        ),
        None
    );
    assert!(
        !crate::agent::stream_completion::record_quota_exhaustion_with_delivery(
            allowed,
            AgentKind::Claude,
            None,
            None,
            false
        )
        .recorded()
    );
    assert!(crate::routing::rate_limit::get_rate_limit_info(&AgentKind::Claude, None).is_none());
}

#[test]
fn claude_rejected_status_uses_the_structured_reset() {
    let allowed = concat!(
        r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt""#,
        r#":1790750400,"rateLimitType":"five_hour","overageStatus":"rejected","overage"#,
        r#"DisabledReason":"org_level_disabled_until","isUsingOverage":false,"unifiedW"#,
        r#"indows":{"five_hour":{"utilization":0.76,"resetsAt":1790750400},"seven_day""#,
        r#":{"utilization":0.7,"resetsAt":1790748000}}},"uuid":"0433b702-fa48-4e68-ae3"#,
        r#"2-414ffeee1b33","session_id":"3eaf9e7b-31e3-482c-bb96-66e14f3bf04a"}"#,
    );
    let mut event: serde_json::Value = serde_json::from_str(allowed).expect("captured JSON");
    event["rate_limit_info"]["status"] = serde_json::json!("rejected");
    let reset = chrono::Utc::now().timestamp() + 7200;
    event["rate_limit_info"]["resetsAt"] = serde_json::json!(reset);
    let temp = tempfile::tempdir().expect("temp dir");
    let _home = crate::support::paths::AidHomeGuard::set(temp.path());
    assert!(
        crate::routing::rate_limit::refusal_on_channel(
            &event.to_string(),
            AgentKind::Claude,
            crate::routing::quota::channel::Channel::CliStream
        )
        .is_some()
    );
    let outcome = crate::agent::stream_completion::record_quota_exhaustion_with_delivery(
        &event.to_string(),
        AgentKind::Claude,
        None,
        None,
        false,
    );
    assert!(
        outcome.should_fail(),
        "rejected event must take quota continuation"
    );
    let info = crate::routing::rate_limit::get_rate_limit_info(&AgentKind::Claude, None).expect("marker");
    let at = crate::routing::rate_limit::parse_recovery_datetime(
        info.recovery_at.as_deref().expect("stated reset"),
    )
    .expect("parse marker");
    assert!((at - chrono::Local::now().naive_local()).num_minutes() >= 119);
    assert!(crate::routing::rate_limit::is_rate_limited(
        &AgentKind::Claude,
        None
    ));
}

#[test]
fn claude_error_result_is_a_refusal_but_assistant_prose_is_not() {
    use crate::routing::quota::channel::Channel;
    let result = r#"{"type":"result","is_error":true,"result":"You've hit your limit"}"#;
    assert_eq!(
        crate::routing::rate_limit::refusal_on_channel(result, AgentKind::Claude, Channel::CliStream),
        Some("You've hit your limit".to_string())
    );
    let prose = serde_json::json!({
        "type": "assistant",
        "message": {"content": [{"type": "text", "text": "You've hit your limit"}]}
    })
    .to_string();
    assert_eq!(
        crate::routing::rate_limit::refusal_on_channel(&prose, AgentKind::Claude, Channel::CliStream),
        None
    );
}

#[test]
fn claude_reset_order_prefers_unix_then_zoned_clock_then_fallback() {
    let reset = chrono::Utc::now().timestamp() + 7200;
    let unix =
        crate::routing::rate_limit::parse_recovery_time(&format!("Claude AI usage limit reached|{reset}"))
            .expect("unix reset");
    assert_eq!(
        unix,
        crate::routing::rate_limit::format_recovery(
            chrono::DateTime::from_timestamp(reset, 0)
                .expect("timestamp")
                .with_timezone(&Local)
                .naive_local()
        )
    );
    assert_eq!(
        crate::routing::rate_limit::parse_recovery_time(&format!(
            "You've hit your limit · resets 5pm (Asia/Shanghai)|{reset}"
        )),
        Some(unix)
    );
    let clock =
        parse_relative_recovery("You've hit your limit · resets 5pm (Asia/Shanghai)").expect("zoned reset");
    let delta = clock - Local::now().naive_local();
    assert!(delta.num_hours() >= 0 && delta.num_hours() <= 24);
    assert_eq!(parse_relative_recovery("You've hit your limit"), None);
}
