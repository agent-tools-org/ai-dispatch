// Completion tail and delivery regression tests moved from the main test file.
// Deps: parent completion helpers and isolated provider markers.

use super::*;

/// The two facts must stay separable. `watcher.rs` clears the rate-limit marker
/// on every Done task, so a run that delivered *and* hit a refusal has to report
/// "recorded, but not a failure" — collapsing them into one bool wiped the
/// outage microseconds after recording it and handed routing back a dead provider.
#[test]
fn a_delivered_run_that_hit_a_refusal_keeps_its_marker() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());
    crate::routing::rate_limit::clear_rate_limit(&crate::types::AgentKind::Qwen, None);

    let mut report = String::from("## Findings\n\n");
    report.push_str(&"Reviewed the adapter and its tests. ".repeat(20));
    report.push_str("\nQuota exhausted: Your token-plan 5-hour quota has been exhausted.");

    let outcome = record_quota_exhaustion_with_delivery(
        &report,
        crate::types::AgentKind::Qwen,
        None,
        None,
        true,
    );
    assert!(outcome.recorded(), "the outage must be recorded");
    assert!(!outcome.should_fail(), "a run that delivered is not a failed task");
    // What watcher.rs consults before clearing.
    assert!(crate::routing::rate_limit::marker_path(&crate::types::AgentKind::Qwen, None).exists());
}

#[test]
fn quota_scan_tail_captures_refusal_before_large_diagnostics() {
    let refusal = "Quota exhausted: Your token-plan 5-hour quota has been exhausted.";
    let diagnostics = "x".repeat(10_000);
    let output = format!("{refusal}\n{diagnostics}");
    let tail = quota_scan_tail(&output);
    assert!(
        quota_line(tail, crate::types::AgentKind::Qwen).is_some(),
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
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());
    let agent = crate::types::AgentKind::Grok;
    crate::routing::rate_limit::clear_rate_limit(&agent, None);

    for line in [
        "I hit a rate limit while reading the file",
        "429",
        "The task is about rate_limit markers",
        "=== AID TASK t-abc DONE (exit 0) ===",
    ] {
        assert!(
            !record_quota_exhaustion(line, agent, None, None).recorded(),
            "grok must not be marked from its own prose: {line}"
        );
    }
    assert!(!crate::routing::rate_limit::is_rate_limited(&agent, None));
}

/// The buffered and PTY watchers pass whatever model the run recorded, which is
/// often nothing. A cursor premium refusal must still land on the premium group
/// rather than the whole agent, so `auto` stays dispatchable.
#[test]
fn a_cursor_premium_refusal_with_no_recorded_model_marks_only_the_premium_pool() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());

    let cursor = crate::types::AgentKind::Cursor;
    crate::routing::rate_limit::clear_all_rate_limits_for_agent(&cursor, None);
    let refusal = "ActionRequiredError: Increase limits for faster responses You're out of \
                   usage. Switch to Auto, or ask your admin to increase your limit to continue.";

    let envelope = serde_json::json!({"type":"error", "message":refusal}).to_string();
    assert!(record_quota_exhaustion(&envelope, cursor, None, None).should_fail());
    assert!(crate::routing::rate_limit::is_group_rate_limited(&cursor, None, "premium"));
    assert!(!crate::routing::rate_limit::is_group_rate_limited(&cursor, None, "auto"));
    assert!(
        !crate::routing::rate_limit::is_rate_limited(&cursor, None),
        "a tier refusal must not write off the whole agent"
    );
}

/// The other direction: the same needle, arriving in the envelope only the CLI
/// can open, must still be recorded.
#[test]
fn the_same_needle_in_a_cli_error_envelope_still_marks() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());
    let cursor = crate::types::AgentKind::Cursor;
    crate::routing::rate_limit::clear_all_rate_limits_for_agent(&cursor, None);

    let output = r#"{"type":"error","message":"quota exceeded for this workspace"}"#;
    assert!(record_quota_exhaustion(output, cursor, None, None).recorded());
    assert!(crate::routing::rate_limit::is_rate_limited(&cursor, None));
}
