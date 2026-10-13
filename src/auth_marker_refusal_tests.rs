// OpenCode structured auth errors and credential-change hold regressions.
// Deps: auth_marker, paths::AidHomeGuard, temporary credential metadata.

use super::*;
use crate::paths::AidHomeGuard;

#[test]
fn opencode_structured_401_sets_auth_marker_refusal_regression() {
    let home = tempfile::tempdir().expect("home");
    let _guard = AidHomeGuard::set(home.path());
    std::fs::create_dir_all(crate::paths::logs_dir()).expect("logs");
    let error = serde_json::json!({"type": "error", "error": {"name": "APIError", "data": {
        "statusCode": 401, "message": "Unauthorized",
        "responseBody": r#"{"errors":[{"code":10000,"message":"Authentication error"}]}"#
    }}})
    .to_string();
    std::fs::write(crate::paths::log_path("t-auth"), &error).expect("log");
    assert!(record_from_run("t-auth", AgentKind::OpenCode));
    assert!(auth_status(AgentKind::OpenCode, None).failed());
    for channel in [Channel::CliStream, Channel::CliStderr] {
        assert!(auth_failure_line(&error, AgentKind::OpenCode, channel).is_some());
        let prose = serde_json::json!({"type": "assistant", "text": error}).to_string();
        assert_eq!(auth_failure_line(&prose, AgentKind::OpenCode, channel), None);
        assert_eq!(
            auth_failure_line("APIError 401 Authentication error", AgentKind::OpenCode, channel),
            None
        );
    }
    clear_on_success(AgentKind::OpenCode);
    assert!(!auth_status(AgentKind::OpenCode, None).failed());
}

#[test]
fn opencode_auth_hold_waits_for_credentials_change_refusal_regression() {
    let home = tempfile::tempdir().expect("home");
    let _guard = AidHomeGuard::set(home.path());
    let at = Local::now();
    record_failure_at(AgentKind::OpenCode, "APIError statusCode 401", at);
    assert!(auth_status_at(AgentKind::OpenCode, None, at + Duration::days(2)).failed());
    let credentials = crate::paths::aid_dir().join("opencode-auth.json");
    std::fs::write(&credentials, "synthetic authentication state").expect("new credentials");
    assert!(!auth_status(AgentKind::OpenCode, None).failed());
    record_failure_at(AgentKind::OpenCode, "APIError statusCode 401", at);
    assert!(auth_status_at(AgentKind::OpenCode, None, at + Duration::days(2)).failed());
    std::fs::remove_file(credentials).expect("remove credentials");
    assert!(!auth_status(AgentKind::OpenCode, None).failed());
}

#[test]
fn opencode_401_billing_and_tool_errors_do_not_set_auth_markers() {
    for value in [
        serde_json::json!({"type": "error", "error": {"name": "APIError", "data": {
            "statusCode": 401, "message": "Insufficient balance. Manage your billing here"}}}),
        serde_json::json!({"type": "tool_result", "error": {"name": "APIError", "data": {
            "statusCode": 401, "message": "Authentication error"}}}),
        serde_json::json!({"type": "error", "error": {"name": "APIError", "data": {
            "statusCode": 429, "message": "Authentication error"}}}),
    ] {
        for channel in [Channel::CliStream, Channel::CliStderr] {
            assert_eq!(
                auth_failure_line(&value.to_string(), AgentKind::OpenCode, channel),
                None
            );
        }
    }
}
