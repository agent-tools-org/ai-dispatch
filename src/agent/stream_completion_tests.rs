// Tests for streaming completion status and quota exhaustion on the success path.
// Deps: super::{record_quota_exhaustion, quota_line, ...}, serde_json::Value.

use super::*;
use serde_json::Value;

fn test_codex_error_envelope(message: &str) -> String {
    serde_json::json!({"type": "error", "message": message}).to_string()
}

#[test]
fn result_is_error_true_fails() {
    let v: Value = serde_json::from_str(
        r#"{"type":"result","subtype":"error_during_execution","is_error":true}"#,
    )
    .unwrap();
    assert!(result_envelope_failed(&v));
}

#[test]
fn result_success_is_error_false_ok() {
    let v: Value = serde_json::from_str(
        r#"{"type":"result","subtype":"success","is_error":false,"result":"ok"}"#,
    )
    .unwrap();
    assert!(!result_envelope_failed(&v));
}

#[test]
fn unknown_result_subtype_is_not_invented_as_failure() {
    let v: Value = serde_json::from_str(
        r#"{"type":"result","subtype":"future_success_shape","is_error":false}"#,
    )
    .unwrap();
    assert!(!result_envelope_failed(&v));
}

#[test]
fn nested_opencode_error_type_fails() {
    let out = r#"{"type":"error","error":{"name":"UnknownError","data":{"message":"x"}}}"#;
    assert_eq!(
        status_from_result_jsonl(out).status,
        TaskStatus::Failed
    );
}

#[test]
fn record_quota_exhaustion_ignores_agent_prose_about_rate_limits() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());
    crate::routing::rate_limit::clear_rate_limit(&crate::types::AgentKind::Cursor, None);

    let report = format!(
        "Conclusion: {}\n",
        "The RPC provider throttles us; we saw a 429 and burned Alchemy credits"
    );
    assert!(!record_quota_exhaustion(&report, crate::types::AgentKind::Cursor, None, None,)
    .recorded());
    assert!(!crate::routing::rate_limit::is_rate_limited(&crate::types::AgentKind::Cursor, None));
}

#[test]
fn record_quota_exhaustion_detects_provider_refusal_templates() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());

    crate::routing::rate_limit::clear_rate_limit(&crate::types::AgentKind::Codex, None);
    let codex_out = test_codex_error_envelope(
        "You have hit your usage limit.",
    );
    assert!(record_quota_exhaustion(&codex_out, crate::types::AgentKind::Codex, None, None,)
    .should_fail());
    assert!(crate::routing::rate_limit::is_rate_limited(&crate::types::AgentKind::Codex, None));
    crate::routing::rate_limit::clear_rate_limit(&crate::types::AgentKind::Codex, None);

    crate::routing::rate_limit::clear_rate_limit(&crate::types::AgentKind::Codex, None);
    let codex_out = format!(
        "You have hit your usage limit. try again at {}.",
        crate::routing::rate_limit::test_future_recovery_time()
    );
    assert!(record_quota_exhaustion(&codex_out, crate::types::AgentKind::Codex, None, None,)
    .should_fail());
    assert!(crate::routing::rate_limit::is_rate_limited(&crate::types::AgentKind::Codex, None));
}

#[test]
fn prose_rate_limit_tokens_are_not_quota_failures() {
    assert!(!crate::routing::rate_limit::is_rate_limit_error(
        "rate_limit_kind now returns AgentKind::Custom for custom agents"
    ));
    assert!(!crate::routing::rate_limit::is_rate_limit_error(
        "We must respect the rate limit on the Base sequencer feed"
    ));
    assert!(!crate::routing::rate_limit::is_rate_limit_error(
        "The parser handles nested arrays correctly"
    ));
}

#[test]
fn explicit_delivery_survives_a_later_quota_refusal() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());
    crate::routing::rate_limit::clear_rate_limit(&crate::types::AgentKind::Codex, None);

    let mut report = String::from("## Findings\n\n");
    report.push_str(&"The audit reviewed rate limits and 429 handling. ".repeat(20));
    report.push_str("\nYou have hit your usage limit.");
    assert!(!record_quota_exhaustion_with_delivery(
        &report,
        crate::types::AgentKind::Codex,
        None,
        None,
        true,
    )
    .should_fail());
    assert!(crate::routing::rate_limit::is_rate_limited(&crate::types::AgentKind::Codex, None));
}

#[test]
fn markdown_shape_does_not_turn_a_refusal_into_delivery() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());
    crate::routing::rate_limit::clear_rate_limit(&crate::types::AgentKind::Codex, None);

    let output = "# Error\nYou have hit your usage limit.";
    assert!(record_quota_exhaustion(output, crate::types::AgentKind::Codex, None, None,)
    .should_fail());
    assert!(crate::routing::rate_limit::is_rate_limited(&crate::types::AgentKind::Codex, None));
}

/// A provider whose refusal wording nobody has captured is undetectable on the
/// prose channel, by design. A bare `429` line is as likely to be a task id the
/// agent printed as a provider status, so guessing here is what marked cursor
/// rate-limited off an audit report twice on 2026-08-06.
#[test]
fn unobserved_providers_are_not_guessed_from_generic_prose() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());

    for agent in [crate::types::AgentKind::Claude, crate::types::AgentKind::Grok] {
        crate::routing::rate_limit::clear_rate_limit(&agent, None);
        assert!(!record_quota_exhaustion("429 Too Many Requests", agent, None, None).recorded());
        assert!(!crate::routing::rate_limit::is_rate_limited(&agent, None));
    }

    // The same shape an agent writes in a report must not mark either.
    crate::routing::rate_limit::clear_rate_limit(&crate::types::AgentKind::Cursor, None);
    for line in ["429", "## Rate Limit", "Task 429", "Rate Limit"] {
        assert!(!record_quota_exhaustion(line, crate::types::AgentKind::Cursor, None, None).recorded());
    }
    assert!(!crate::routing::rate_limit::is_rate_limited(&crate::types::AgentKind::Cursor, None));
}

#[test]
fn quota_line_anchors_on_signature_needle_without_quota_word() {
    let output = "402 payment required: reload your tokens";
    let line = quota_line(output, crate::types::AgentKind::Droid).expect("droid line");
    assert!(line.contains("reload your tokens"));
    assert!(line.starts_with("402"));
}

/// `~/.aid/rate-limit-copilot` began `sage\":\"You have exceeded` — the marker
/// held a fragment sliced mid-token, because the window start was a fixed 40
/// characters back from the anchor and that lands inside `"message\":\"`.
/// The recorded message must be the provider's own sentence.
#[test]
fn quota_line_records_a_clean_refusal_not_a_mid_token_fragment() {
    let event = r#"{"type":"error","errorCode":"quota_exceeded","message":"You have exceeded your monthly quota","code":"quota_exceeded","requestFingerprint":{"messageCount":2}}"#;
    let line = quota_line(event, crate::types::AgentKind::Copilot).expect("copilot line");
    assert_eq!(line, "You have exceeded your monthly quota");
}

/// The grok refusal arrives wrapped in a JSON body too.
#[test]
fn quota_line_unwraps_the_grok_payment_refusal() {
    let event = r#"  "message": "API error (status 402 Payment Required): Grok Build usage balance exhausted","#;
    let line = quota_line(event, crate::types::AgentKind::Grok).expect("grok line");
    assert_eq!(
        line,
        "API error (status 402 Payment Required): Grok Build usage balance exhausted"
    );
}

/// Unwrapping must not cost the reset time a provider does state: codex's
/// refusal is plain text and has to survive whole.
#[test]
fn quota_line_keeps_a_stated_reset_time_on_plain_text() {
    let message = "You've hit your usage limit. Visit https://chatgpt.com/codex/settings/usage \
                   to purchase more credits or try again at Aug 11th, 2026 2:23 PM.";
    let line = quota_line(message, crate::types::AgentKind::Codex).expect("codex line");
    assert!(line.contains("try again at Aug 11th, 2026 2:23 PM"), "got {line}");
}

/// A JSON-wrapped refusal must keep its embedded reset time as well, so the
/// marker is held to the stated window rather than a class default.
#[test]
fn quota_line_keeps_a_reset_time_embedded_in_json() {
    let event = test_codex_error_envelope(
        "You have hit your usage limit. Please try again at Aug 12th, 2099 10:12 AM.\n\nPlease retry",
    );
    let attributable = crate::routing::quota::channel::provider_attributable(
        &event,
        crate::types::AgentKind::Codex,
        crate::routing::quota::channel::Channel::CliStream,
    )
    .all();
    let line = quota_line(&attributable, crate::types::AgentKind::Codex).expect("codex line");
    assert!(line.starts_with("You have hit your usage limit."), "got {line}");
    assert!(line.contains("try again at Aug 12th, 2099 10:12 AM"), "got {line}");
    assert!(!line.contains('\\'), "JSON escapes must not survive: {line}");
}

/// The live false positive of 2026-08-07, end to end.
///
/// A cursor audit task read `src/agent/cursor_tests.rs:142` and quoted it into
/// its report. aid wrote `~/.aid/rate-limit-cursor`, held cursor until the next
/// day and recorded the task FAILED with exit code 0, while cursor served the
/// whole time. The report reaches the watcher as assistant envelopes, which is
/// where the split now drops it.
#[test]
fn a_report_quoting_this_repos_own_fixture_never_marks_the_agent() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());
    let cursor = crate::types::AgentKind::Cursor;
    crate::routing::rate_limit::clear_all_rate_limits_for_agent(&cursor, None);

    let quoted = "assert_rate_limit(r#\"{\"type\":\"error\",\"message\":\"quota exceeded for \
                  this workspace\"}\"#, true);\n====\ncommit 7881e2d";
    let output = format!(
        "{}\n{}\n",
        serde_json::json!({"type": "system", "subtype": "init", "model": "composer-2.5"}),
        serde_json::json!({
            "type": "assistant",
            "message": {"content": [{"type": "text", "text": quoted}]}
        })
    );

    assert!(!record_quota_exhaustion(&output, cursor, None, None).recorded());
    assert!(!crate::routing::rate_limit::is_rate_limited(&cursor, None));
    assert!(!crate::routing::rate_limit::is_group_rate_limited(&cursor, None, "premium"));
}

/// The same guarantee over every signature we ship, so a needle added later
/// inherits it: nothing an agent can write into its own message becomes a hold.

#[path = "stream_completion_tests_more_1.rs"]
mod more;
