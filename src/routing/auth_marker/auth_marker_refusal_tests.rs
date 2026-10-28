// OpenCode structured auth errors and credential-change hold regressions.
// Deps: auth_marker, paths::AidHomeGuard, temporary credential metadata.

use super::*;


#[test]
fn opencode_structured_401_sets_auth_marker_refusal_regression() {
    let _home = crate::support::test_env::TestHome::new();
    std::fs::create_dir_all(crate::support::paths::logs_dir()).expect("logs");
    let error = serde_json::json!({"type": "error", "error": {"name": "APIError", "data": {
        "statusCode": 401, "message": "Unauthorized",
        "responseBody": r#"{"errors":[{"code":10000,"message":"Authentication error"}]}"#
    }}})
    .to_string();
    std::fs::write(crate::support::paths::log_path("t-auth"), &error).expect("log");
    let store = failed_opencode_store("t-auth");
    let task = store.get_task("t-auth").expect("task").expect("task");
    let event = crate::agent::get_agent(AgentKind::OpenCode)
        .parse_event(&task.id, &error).expect("decoded provider event");
    store.insert_event(&event).expect("event");
    assert!(record_from_run("t-auth", AgentKind::OpenCode, &store));
    assert!(auth_status(AgentKind::OpenCode, None).failed());
    for channel in [Channel::CliStream, Channel::CliStderr] {
        assert_eq!(auth_failure_line(&error, AgentKind::OpenCode, channel).is_some(),
            channel == Channel::CliStream);
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
    let _home = crate::support::test_env::TestHome::new();
    let at = Local::now();
    record_failure_at(AgentKind::OpenCode, "APIError statusCode 401", at);
    assert!(auth_status_at(AgentKind::OpenCode, None, at + Duration::days(2)).failed());
    let credentials = crate::support::paths::aid_dir().join("opencode-auth.json");
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

fn assert_run_has_no_auth_marker(output: &str, stderr: bool) {
    let _home = crate::support::test_env::TestHome::new();
    crate::support::paths::ensure_dirs().expect("directories");
    std::fs::write(crate::support::paths::log_path("t-fixture"), output).expect("log");
    if stderr {
        std::fs::write(crate::support::paths::stderr_path("t-fixture"), output).expect("stderr");
    }
    let store = failed_opencode_store("t-fixture");
    let task = store.get_task("t-fixture").expect("task").expect("task");
    if !stderr {
        if let Some(event) = crate::agent::get_agent(AgentKind::OpenCode).parse_event(&task.id, output) {
            store.insert_event(&event).expect("event");
        }
    }
    assert!(!record_from_run("t-fixture", AgentKind::OpenCode, &store));
    assert!(!marker_path(AgentKind::OpenCode).exists());
}

#[test]
fn tool_scoped_401_run_does_not_mark_auth_fix_regression() {
    for field in ["tool_call", "tool_use_id", "toolCallId"] {
        let mut error = serde_json::json!({"type": "error", "error": {"name": "APIError",
            "data": {"statusCode": 401, "message": "Unauthorized"}}});
        error[field] = serde_json::json!({});
        assert_run_has_no_auth_marker(&error.to_string(), false);
    }
}

#[test]
fn stderr_401_fixture_and_replayed_log_do_not_mark_auth_fix_regression() {
    let error = serde_json::json!({"type": "error", "error": {"name": "APIError",
        "data": {"statusCode": 401, "message": "Unauthorized"}}});
    assert_run_has_no_auth_marker(&error.to_string(), true);
}

#[test]
fn assistant_401_error_run_does_not_mark_auth_fix_regression() {
    let error = serde_json::json!({"type": "assistant", "error": {"name": "APIError",
        "data": {"statusCode": 401, "message": "Unauthorized"}}});
    assert_run_has_no_auth_marker(&error.to_string(), false);
}

fn failed_opencode_store(task_id: &str) -> crate::store::Store {
    let store = crate::store::Store::open_memory().expect("store");
    store.db().execute(
        "INSERT INTO tasks (id, agent, prompt, status, exit_code, created_at)
         VALUES (?1, 'opencode', 'auth fixture', 'failed', 1, ?2)",
        rusqlite::params![task_id, Local::now().to_rfc3339()],
    ).expect("task");
    store
}
