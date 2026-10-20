// Source-attribution regression tests for completion-time quota handling.
// Compares unsplit PTY prose with a real provider error envelope.

use super::*;
use crate::routing::rate_limit;
use crate::types::AgentKind;

#[test]
fn pty_needs_human_excerpt_never_fails_a_successful_run() {
    let home = tempfile::tempdir().expect("home");
    let _guard = crate::support::paths::AidHomeGuard::set(home.path());
    let text = "Insufficient balance. Manage your billing here";
    let outcome =
        record_quota_exhaustion_with_delivery(text, AgentKind::OpenCode, None, None, false);
    assert!(outcome.recorded());
    assert!(!outcome.should_fail());
    let marker = std::fs::read_to_string(rate_limit::marker_path(&AgentKind::OpenCode, None))
        .expect("excerpt marker");
    assert!(marker.contains(text));
    assert_eq!(
        crate::routing::availability::stored_hold(&marker, &AgentKind::OpenCode),
        crate::routing::availability::StoredHold::Transient
    );
    let trusted = serde_json::json!({"type":"error", "message":text}).to_string();
    assert!(record_quota_exhaustion(&trusted, AgentKind::OpenCode, None, None).should_fail());
    let info = rate_limit::get_rate_limit_info(&AgentKind::OpenCode, None).expect("trusted hold");
    assert!(info.needs_human);
    record_quota_exhaustion(text, AgentKind::OpenCode, None, None);
    assert!(
        rate_limit::get_rate_limit_info(&AgentKind::OpenCode, None)
            .expect("kept hold")
            .needs_human
    );
}

#[test]
fn unsplit_monthly_refusal_gets_only_a_transient_hold() {
    let home = tempfile::tempdir().expect("home");
    let _guard = crate::support::paths::AidHomeGuard::set(home.path());
    let text = include_str!("../../tests/fixtures/cursor-usage-limit-refusal.txt");
    let outcome = record_quota_exhaustion(text, AgentKind::Cursor, None, Some("auto"));
    assert!(outcome.recorded());
    assert!(!outcome.should_fail());
    let marker = std::fs::read_to_string(rate_limit::group_marker_path(
        &AgentKind::Cursor,
        None,
        "auto",
    ))
    .expect("marker");
    assert!(marker.contains("source: untrusted"));
    assert!(rate_limit::marker_field(&marker, "recovery_at: ").is_none());
}

#[test]
fn typed_non_quota_codex_error_overrules_quoted_quota_text() {
    let home = tempfile::tempdir().expect("home");
    let _guard = crate::support::paths::AidHomeGuard::set(home.path());
    let event = serde_json::json!({"type":"error", "error":{
        "codexErrorInfo":"contextWindowExceeded", "message":"hit your usage limit"}});
    assert!(!record_quota_exhaustion(&event.to_string(), AgentKind::Codex, None, None).recorded());
}

#[test]
fn unwrapped_claude_reset_preserves_an_excerpt_without_a_long_hold() {
    let home = tempfile::tempdir().expect("home");
    let _guard = crate::support::paths::AidHomeGuard::set(home.path());
    let reset = chrono::Utc::now().timestamp() + 7 * 24 * 3600;
    let message = format!("You've hit your limit|{reset}");
    let outcome = record_quota_exhaustion(&message, AgentKind::Claude, None, None);
    assert!(outcome.recorded());
    assert!(!outcome.should_fail());
    let info = rate_limit::get_rate_limit_info(&AgentKind::Claude, None).expect("excerpt");
    assert_eq!(info.message.as_deref(), Some(message.as_str()));
    assert!(info.recovery_at.is_none());
    assert!(!info.needs_human);
}

#[test]
fn unsplit_codex_refusal_records_without_failing() {
    let home = tempfile::tempdir().expect("home");
    let _guard = crate::support::paths::AidHomeGuard::set(home.path());
    let outcome = record_quota_exhaustion(
        "You have hit your usage limit.", AgentKind::Codex, None, None,
    );
    assert!(outcome.recorded());
    assert!(!outcome.should_fail());
    let path = rate_limit::marker_path(&AgentKind::Codex, None);
    assert!(path.exists());
    let marker = std::fs::read_to_string(path).expect("excerpt marker");
    assert_eq!(
        crate::routing::availability::stored_hold(&marker, &AgentKind::Codex),
        crate::routing::availability::StoredHold::Transient
    );
}
