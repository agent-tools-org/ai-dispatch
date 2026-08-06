// Tests for streaming completion status and quota exhaustion on the success path.
// Deps: super::{record_quota_exhaustion, quota_line, ...}, serde_json::Value.

use super::*;
use serde_json::Value;

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
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    crate::rate_limit::clear_rate_limit(&crate::types::AgentKind::Cursor);

    let report = format!(
        "Conclusion: {}\n",
        "The RPC provider throttles us; we saw a 429 and burned Alchemy credits"
    );
    assert!(!record_quota_exhaustion(
        &report,
        crate::types::AgentKind::Cursor,
        None,
    )
    .recorded());
    assert!(!crate::rate_limit::is_rate_limited(&crate::types::AgentKind::Cursor));
}

#[test]
fn record_quota_exhaustion_detects_provider_refusal_templates() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());

    crate::rate_limit::clear_rate_limit(&crate::types::AgentKind::Qwen);
    let qwen_out = "Quota exhausted: Your token-plan 5-hour quota has been exhausted.";
    assert!(record_quota_exhaustion(
        qwen_out,
        crate::types::AgentKind::Qwen,
        None,
    )
    .should_fail());
    assert!(crate::rate_limit::is_rate_limited(&crate::types::AgentKind::Qwen));
    crate::rate_limit::clear_rate_limit(&crate::types::AgentKind::Qwen);

    crate::rate_limit::clear_rate_limit(&crate::types::AgentKind::Codex);
    let codex_out = "You have hit your usage limit. try again at Mar 21st, 2099 2:27 PM.";
    assert!(record_quota_exhaustion(
        codex_out,
        crate::types::AgentKind::Codex,
        None,
    )
    .should_fail());
    assert!(crate::rate_limit::is_rate_limited(&crate::types::AgentKind::Codex));
}

#[test]
fn prose_rate_limit_tokens_are_not_quota_failures() {
    assert!(!crate::rate_limit::is_rate_limit_error(
        "rate_limit_kind now returns AgentKind::Custom for custom agents"
    ));
    assert!(!crate::rate_limit::is_rate_limit_error(
        "We must respect the rate limit on the Base sequencer feed"
    ));
    assert!(!crate::rate_limit::is_rate_limit_error(
        "The parser handles nested arrays correctly"
    ));
}

#[test]
fn record_quota_exhaustion_marks_but_does_not_fail_substantive_deliverable() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    crate::rate_limit::clear_rate_limit(&crate::types::AgentKind::Qwen);

    let mut report = String::from("## Findings\n\n");
    report.push_str(&"The audit reviewed rate limits and 429 handling. ".repeat(20));
    report.push_str("\nQuota exhausted: Your token-plan 5-hour quota has been exhausted.");
    assert!(!record_quota_exhaustion(
        &report,
        crate::types::AgentKind::Qwen,
        None,
    )
    .should_fail());
    assert!(crate::rate_limit::is_rate_limited(&crate::types::AgentKind::Qwen));
}

#[test]
fn record_quota_exhaustion_marks_refusal_even_behind_markdown_heading() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    crate::rate_limit::clear_rate_limit(&crate::types::AgentKind::Qwen);

    let output = "# Error\nQuota exhausted: Your token-plan 5-hour quota has been exhausted.";
    assert!(!record_quota_exhaustion(
        output,
        crate::types::AgentKind::Qwen,
        None,
    )
    .should_fail());
    assert!(crate::rate_limit::is_rate_limited(&crate::types::AgentKind::Qwen));
}

/// A provider whose refusal wording nobody has captured is undetectable on the
/// prose channel, by design. A bare `429` line is as likely to be a task id the
/// agent printed as a provider status, so guessing here is what marked cursor
/// rate-limited off an audit report twice on 2026-08-06.
#[test]
fn unobserved_providers_are_not_guessed_from_generic_prose() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());

    for agent in [crate::types::AgentKind::Claude, crate::types::AgentKind::Grok] {
        crate::rate_limit::clear_rate_limit(&agent);
        assert!(!record_quota_exhaustion("429 Too Many Requests", agent, None).recorded());
        assert!(!crate::rate_limit::is_rate_limited(&agent));
    }

    // The same shape an agent writes in a report must not mark either.
    crate::rate_limit::clear_rate_limit(&crate::types::AgentKind::Cursor);
    for line in ["429", "## Rate Limit", "Task 429", "Rate Limit"] {
        assert!(!record_quota_exhaustion(line, crate::types::AgentKind::Cursor, None).recorded());
    }
    assert!(!crate::rate_limit::is_rate_limited(&crate::types::AgentKind::Cursor));
}

#[test]
fn quota_line_anchors_on_signature_needle_without_quota_word() {
    let output = "402 payment required: reload your tokens";
    let line = quota_line(output, crate::types::AgentKind::Droid).expect("droid line");
    assert!(line.contains("reload your tokens"));
    assert!(line.starts_with("402"));
}

#[test]
fn record_quota_exhaustion_ignores_signature_source_citations() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    crate::rate_limit::clear_rate_limit(&crate::types::AgentKind::OpenCode);

    let output = "QuotaSignature { agent: AgentKind::OpenCode, needle: \"insufficient balance\", fallback_minutes: 1440 },";
    assert!(!record_quota_exhaustion(
        output,
        crate::types::AgentKind::OpenCode,
        None,
    )
    .recorded());
    assert!(!crate::rate_limit::is_rate_limited(&crate::types::AgentKind::OpenCode));
}

/// The two facts must stay separable. `watcher.rs` clears the rate-limit marker
/// on every Done task, so a run that delivered *and* hit a refusal has to report
/// "recorded, but not a failure" — collapsing them into one bool wiped the
/// outage microseconds after recording it and handed routing back a dead provider.
#[test]
fn a_delivered_run_that_hit_a_refusal_keeps_its_marker() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    crate::rate_limit::clear_rate_limit(&crate::types::AgentKind::Qwen);

    let mut report = String::from("## Findings\n\n");
    report.push_str(&"Reviewed the adapter and its tests. ".repeat(20));
    report.push_str("\nQuota exhausted: Your token-plan 5-hour quota has been exhausted.");

    let outcome = record_quota_exhaustion(&report, crate::types::AgentKind::Qwen, None);
    assert!(outcome.recorded(), "the outage must be recorded");
    assert!(!outcome.should_fail(), "a run that delivered is not a failed task");
    // What watcher.rs consults before clearing.
    assert!(crate::rate_limit::is_rate_limited(&crate::types::AgentKind::Qwen));
}
