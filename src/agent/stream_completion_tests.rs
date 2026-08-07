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
    let event = r#"{"type":"result","text":"Quota exhausted: Your token-plan 1-week quota has been exhausted. The quota will reset at 08-12 10:12:00 UTC.\n\nPlease retry"}"#;
    let line = quota_line(event, crate::types::AgentKind::Qwen).expect("qwen line");
    assert!(line.starts_with("Quota exhausted:"), "got {line}");
    assert!(line.contains("reset at 08-12 10:12:00 UTC"), "got {line}");
    assert!(!line.contains('\\'), "JSON escapes must not survive: {line}");
}

#[test]
fn record_quota_exhaustion_ignores_signature_source_citations() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    crate::rate_limit::clear_rate_limit(&crate::types::AgentKind::OpenCode);

    let output = "QuotaSignature { agent: AgentKind::OpenCode, needle: \"insufficient balance\", recovery: QuotaRecovery::NeedsHuman },";
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

#[test]
fn quota_scan_tail_captures_refusal_before_large_diagnostics() {
    let refusal = "Quota exhausted: Your token-plan 5-hour quota has been exhausted.";
    let diagnostics = "x".repeat(10_000);
    let output = format!("{refusal}\n{diagnostics}");
    let tail = quota_scan_tail(&output);
    assert!(
        agent_prose_quota_match(tail, crate::types::AgentKind::Qwen),
        "refusal must be preserved even when followed by >4 KB of diagnostics"
    );
}

#[test]
fn quota_scan_tail_aligns_start_to_line_boundary() {
    let refusal = "Quota exhausted: Your token-plan 5-hour quota has been exhausted.";
    let prefix = "header line\n";
    let split_line_header = "split line start ";
    let split_line_y = "y".repeat(100);

    let mut suffix = String::with_capacity(65_440);
    while suffix.len() < 65_440 {
        suffix.push_str("trailing diagnostic line...\n");
    }
    suffix.truncate(65_440);

    let output = format!("{prefix}{split_line_header}{split_line_y}\n{refusal}\n{suffix}");
    assert_eq!(output.len(), 65_636);
    let raw_start = output.len() - 65_536;
    assert_eq!(raw_start, 100);
    assert_ne!(output.as_bytes()[raw_start - 1], b'\n');

    let tail = quota_scan_tail(&output);
    let first_line = tail.lines().next().unwrap_or("");
    assert_eq!(
        first_line,
        format!("{split_line_header}{split_line_y}"),
        "window start must rewind to line boundary and keep full line"
    );
    assert!(
        !first_line.starts_with('y'),
        "window start must align to line boundary and not start mid-line"
    );
}

#[test]
fn quota_scan_tail_keeps_line_when_start_lands_on_line_boundary() {
    let refusal = "Quota exhausted: Your token-plan 5-hour quota has been exhausted.";
    let tail_bytes = 65_536;
    let refusal_with_newline_len = refusal.len() + 1;
    let suffix_len_needed = tail_bytes - refusal_with_newline_len;

    let mut suffix = String::with_capacity(suffix_len_needed);
    while suffix.len() + 2 <= suffix_len_needed {
        suffix.push_str("a\n");
    }
    while suffix.len() < suffix_len_needed {
        suffix.push('a');
    }

    let prefix = "line\n".repeat(100);
    let output = format!("{prefix}{refusal}\n{suffix}");

    let raw_start = output.len() - tail_bytes;
    assert_eq!(raw_start, prefix.len());
    assert_eq!(output.as_bytes()[raw_start - 1], b'\n');

    let tail = quota_scan_tail(&output);
    let first_line = tail.lines().next().unwrap_or("");
    assert_eq!(
        first_line, refusal,
        "when start lands exactly on line boundary, the first line must be kept"
    );
}

#[test]
fn quota_scan_tail_rewind_is_bounded_when_output_has_no_newline() {
    // A single line longer than two windows: rewinding to its start would scan
    // everything. The raw offset stands instead.
    let output = "z".repeat(200_000);
    let tail = quota_scan_tail(&output);
    assert!(
        tail.len() <= 65_536,
        "rewind must not expand the window past one extra budget, got {}",
        tail.len()
    );
}

#[test]
fn buffered_grok_prose_about_rate_limits_never_marks_it() {
    // grok has no anchored signature, so nothing it writes about quotas may mark
    // it. This is the invariant that makes wiring record_quota_exhaustion into
    // the buffered watcher safe for grok, whose buffer also carries aid's own
    // terminal sentinel and echoed idle nudges.
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let agent = crate::types::AgentKind::Grok;
    crate::rate_limit::clear_rate_limit(&agent);

    for line in [
        "I hit a rate limit while reading the file",
        "429",
        "The task is about rate_limit markers",
        "=== AID TASK t-abc DONE (exit 0) ===",
    ] {
        assert!(
            !record_quota_exhaustion(line, agent, None).recorded(),
            "grok must not be marked from its own prose: {line}"
        );
    }
    assert!(!crate::rate_limit::is_rate_limited(&agent));
}
