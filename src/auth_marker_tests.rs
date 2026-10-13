// Auth observation expiry, successful-run clearing, and raw-channel regressions.
// Uses auth_marker helpers and isolated AID homes.

use super::*;
use crate::paths::AidHomeGuard;

const GROK_ERROR: &str = concat!(
    r#"{"type":"error","message":"Not signed in. To authenticate without a browser, run:"#,
    r#"\n  grok login --device-code"}"#,
);

#[test]
fn no_evidence_is_unknown() {
    let home = tempfile::tempdir().expect("home");
    let _guard = AidHomeGuard::set(home.path());
    let status = auth_status(AgentKind::Grok, None);
    assert_eq!(status.state, "unknown");
    assert_eq!(status.observed_at, None);
}

#[test]
fn not_signed_in_run_sets_failed_with_observed_time() {
    let home = tempfile::tempdir().expect("home");
    let _guard = AidHomeGuard::set(home.path());
    std::fs::create_dir_all(crate::paths::logs_dir()).expect("logs");
    std::fs::write(crate::paths::log_path("t-auth"), format!("{GROK_ERROR}\n")).expect("log");
    assert!(record_from_run(
        "t-auth",
        AgentKind::Grok,
        &crate::store::Store::open_memory().expect("store")
    ));
    let status = auth_status(AgentKind::Grok, None);
    assert_eq!(status.state, "failed");
    assert!(status.observed_at.is_some());
    assert!(
        status
            .message
            .as_deref()
            .is_some_and(|m| m.starts_with("Not signed in"))
    );
    assert_eq!(auth_status(AgentKind::Codex, None).state, "unknown");
}

#[test]
fn failed_verdict_expires_after_ttl() {
    let home = tempfile::tempdir().expect("home");
    let _guard = AidHomeGuard::set(home.path());
    let at = Local::now() - Duration::seconds(AUTH_FAILED_TTL_SECS + 5);
    record_failure_at(AgentKind::Grok, "Not signed in.", at);
    assert_eq!(auth_status(AgentKind::Grok, None).state, "unknown");
    let fresh = Local::now() - Duration::seconds(AUTH_FAILED_TTL_SECS - 60);
    record_failure_at(AgentKind::Grok, "Not signed in.", fresh);
    assert_eq!(auth_status(AgentKind::Grok, None).state, "failed");
    let later = fresh + Duration::seconds(AUTH_FAILED_TTL_SECS);
    assert_eq!(
        auth_status_at(AgentKind::Grok, None, later).state,
        "unknown"
    );
}

#[test]
fn successful_run_clears_failed_verdict() {
    let home = tempfile::tempdir().expect("home");
    let _guard = AidHomeGuard::set(home.path());
    record_failure_at(
        AgentKind::Claude,
        "Invalid API key · Please run /login",
        Local::now(),
    );
    assert!(auth_status(AgentKind::Claude, None).failed());
    clear_on_success(AgentKind::Claude);
    assert_eq!(auth_status(AgentKind::Claude, None).state, "unknown");
}

#[test]
fn unmatched_or_missing_output_never_marks_failed() {
    let home = tempfile::tempdir().expect("home");
    let _guard = AidHomeGuard::set(home.path());
    assert!(!record_from_run(
        "t-missing",
        AgentKind::Grok,
        &crate::store::Store::open_memory().expect("store")
    ));
    let quoted = r#"{"type":"assistant","message":{"content":[{"text":"Not signed in"}]}}"#;
    assert_eq!(
        auth_failure_line(quoted, AgentKind::Grok, Channel::CliStream),
        None
    );
    assert_eq!(
        auth_failure_line("Not signed in", AgentKind::Codex, Channel::CliStderr),
        None
    );
    assert_eq!(auth_status(AgentKind::Grok, None).state, "unknown");
}
