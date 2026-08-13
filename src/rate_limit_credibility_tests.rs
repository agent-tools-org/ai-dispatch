// Quota-marker credibility tests for provider scope and reset preservation.
// Exports: regression tests for OpenCode attribution and long ISO refusals.
// Deps: crate::rate_limit, crate::agent::stream_completion, chrono.

use super::*;
use crate::types::AgentKind;

fn isolated() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(temp.path().join(".aid")).expect("aid dir");
    temp
}

#[test]
fn opencode_model_refusal_holds_only_the_nvidia_provider() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());
    let agent = AgentKind::OpenCode;

    crate::agent::stream_completion::record_quota_exhaustion(
        "Insufficient balance. Manage your billing here.",
        agent,
        None,
        Some("nvidia/llama-4-maverick"),
    );

    assert!(is_group_rate_limited(&agent, None, "nvidia"));
    assert!(!is_rate_limited(&agent, None));
    assert_eq!(active_group_holds(&agent, None).len(), 1);
    assert!(clear_all_rate_limits_for_agent(&agent, None));
    assert!(!is_group_rate_limited(&agent, None, "nvidia"));
}

#[test]
fn opencode_provider_route_markers_are_case_insensitive() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());
    let agent = AgentKind::OpenCode;

    mark_rate_limited_for_model(
        &agent,
        None,
        Some("OpenCode/deepseek-v4-pro"),
        "Insufficient balance.",
    );

    assert!(is_group_rate_limited(&agent, None, "opencode"));
    assert!(dispatch_blocking_hold_for_model(&agent, None, Some("opencode/other-model"))
        .is_some());
}

#[test]
fn unknown_opencode_provider_is_recorded_and_stays_conservative() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());
    let agent = AgentKind::OpenCode;

    mark_rate_limited_for_message(&agent, None, "Insufficient balance.");

    assert!(is_rate_limited(&agent, None));
    let content = std::fs::read_to_string(marker_path(&agent, None)).expect("marker");
    assert!(content.contains("provider: unknown"), "{content}");
}

#[test]
fn explicitly_named_opencode_provider_is_narrowed_without_a_model() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());
    let agent = AgentKind::OpenCode;

    mark_rate_limited_for_message(
        &agent,
        None,
        r#"{"providerID":"nvidia","message":"Insufficient balance."}"#,
    );

    assert!(is_group_rate_limited(&agent, None, "nvidia"));
    assert!(!is_rate_limited(&agent, None));
}

#[test]
fn explicitly_unknown_opencode_provider_stays_agent_wide() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());
    let agent = AgentKind::OpenCode;

    mark_rate_limited_for_message(
        &agent,
        None,
        r#"{"provider":"unknown","message":"Insufficient balance."}"#,
    );

    assert!(is_rate_limited(&agent, None));
    assert!(!is_group_rate_limited(&agent, None, "unknown"));
}

#[test]
fn opencode_error_for_one_provider_does_not_hold_another_model_route() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());
    let agent = AgentKind::OpenCode;
    let error = serde_json::json!({
        "type": "error",
        "providerID": "opencode",
        "error": {"data": {"message": "Insufficient balance. Manage your billing here"}}
    });

    mark_rate_limited_for_value(
        &agent,
        None,
        &error,
        "Insufficient balance. Manage your billing here",
    );

    assert!(!is_rate_limited(&agent, None));
    assert!(dispatch_blocking_hold_for_model(&agent, None, Some("opencode-go/deepseek-v4-pro"))
        .is_none());
}

#[test]
fn opencode_error_still_blocks_the_provider_that_refused() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());
    let agent = AgentKind::OpenCode;
    let error = serde_json::json!({
        "type": "error",
        "providerID": "opencode",
        "error": {"data": {"message": "Insufficient balance. Manage your billing here"}}
    });

    mark_rate_limited_for_value(
        &agent,
        None,
        &error,
        "Insufficient balance. Manage your billing here",
    );

    assert!(is_group_rate_limited(&agent, None, "opencode"));
    assert!(dispatch_blocking_hold_for_model(&agent, None, Some("opencode/deepseek-v4-pro"))
        .is_some());
}

#[test]
fn opencode_provider_key_beats_provider_text_in_error_message() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());
    let agent = AgentKind::OpenCode;
    let error = serde_json::json!({
        "type": "error",
        "error": {"data": {"message": "providerID: fake-provider/model refused the request: Insufficient balance"}},
        "providerID": "opencode"
    });

    mark_rate_limited_for_value(
        &agent,
        None,
        &error,
        "Insufficient balance. Manage your billing here",
    );

    assert!(is_group_rate_limited(&agent, None, "opencode"));
    assert!(!is_group_rate_limited(&agent, None, "fake-provider"));
    assert!(!is_rate_limited(&agent, None));
}

#[test]
fn opencode_missing_provider_key_stays_agent_wide() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());
    let agent = AgentKind::OpenCode;
    let error = serde_json::json!({
        "type": "error",
        "error": {"data": {"message": "provider opencode-go/deepseek-v4-pro refused the request: Insufficient balance"}}
    });

    mark_rate_limited_for_value(
        &agent,
        None,
        &error,
        "Insufficient balance. Manage your billing here",
    );

    assert!(is_rate_limited(&agent, None));
    assert!(!is_group_rate_limited(&agent, None, "opencode-go"));
}

#[test]
fn long_refusal_keeps_an_iso_reset_timestamp_after_the_old_cutoff() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());
    let prefix = "diagnostic detail ".repeat(20);
    let message = format!("{prefix} Insufficient balance; resetAt=2099-01-02T03:04:05Z");

    mark_rate_limited(&AgentKind::OpenCode, None, &message);

    let info = get_rate_limit_info(&AgentKind::OpenCode, None).expect("marker");
    assert!(info.recovery_at.is_some(), "{info:?}");
    assert!(info.message.is_some_and(|stored| stored.len() > 200));
}

/// The production case, with the event shape actually emitted by the CLI.
///
/// Measured 2026-08-13 against the live out-of-balance provider:
///   opencode run --model opencode/deepseek-v4-pro --format json 'hi'
/// The error event carries NO `providerID` at any depth — the field exists only
/// in the human log format. Attribution therefore cannot come from the refusal;
/// it comes from the route aid dispatched. A fixture that invents `providerID`
/// passes through the parsed-key path and proves nothing about this case.
#[test]
fn real_opencode_refusal_without_provider_key_scopes_to_dispatched_route() {
    let temp = isolated();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());
    let agent = AgentKind::OpenCode;
    let error = serde_json::json!({
        "type": "error",
        "timestamp": 1786593837748i64,
        "sessionID": "ses_006b585dfffevIIzdbtftHpYJO",
        "error": {
            "name": "APIError",
            "data": {
                "message": "Insufficient balance. Manage your billing here: https://opencode.ai/workspace/wrk_x/billing",
                "statusCode": 401,
                "isRetryable": false,
                "metadata": {"url": "https://opencode.ai/zen/v1/chat/completions"}
            }
        }
    });

    mark_rate_limited_for_model_value(
        &agent,
        None,
        Some("opencode/deepseek-v4-pro"),
        &error,
        "Insufficient balance. Manage your billing here",
    );

    assert!(
        is_group_rate_limited(&agent, None, "opencode"),
        "the dispatched provider must be held"
    );
    assert!(
        !is_rate_limited(&agent, None),
        "one provider's refusal must not hold the whole CLI"
    );
    assert!(
        dispatch_blocking_hold_for_model(&agent, None, Some("opencode-go/deepseek-v4-pro")).is_none(),
        "the healthy sibling provider must stay dispatchable"
    );
    assert!(
        dispatch_blocking_hold_for_model(&agent, None, Some("opencode/other-model")).is_some(),
        "another model on the refusing provider must stay blocked"
    );
}
