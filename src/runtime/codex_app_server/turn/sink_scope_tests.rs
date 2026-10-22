// App-server sink regression coverage for typed refusal and snapshot decisions.
// Exercises the real sink and Store without a subprocess or external provider.

use super::*;
use crate::runtime::codex_app_server::fixture;

#[test]
fn typed_usage_refusal_sets_quota_even_without_a_text_needle() {
    let fixture = fixture::Fixture::new("complete");
    let run = run(&fixture);
    let mut sink = Sink::new(&run).expect("sink");
    let mut events = crate::agent::codex::app_server::Events::default();
    let notification = serde_json::json!({"method":"error", "params":{"error":{
        "message":"Allowance unavailable", "codexErrorInfo":"usageLimitExceeded"}}});
    for event in events.notification(&notification) {
        sink.event(event).expect("notification");
    }
    assert!(sink.quota);
    assert!(crate::routing::rate_limit::marker_path(&AgentKind::Codex, None).exists());
}

#[test]
fn typed_other_error_does_not_match_quoted_refusal_text() {
    let fixture = fixture::Fixture::new("complete");
    let run = run(&fixture);
    let mut sink = Sink::new(&run).expect("sink");
    sink.event(serde_json::json!({"type":"turn.failed", "error":{
        "message":"quoted: hit your usage limit", "codexErrorInfo":"contextWindowExceeded"}}))
        .expect("event");
    assert!(!sink.quota);
    assert!(!crate::routing::rate_limit::marker_path(&AgentKind::Codex, None).exists());
    let reset = chrono::Utc::now().timestamp() + 7200;
    sink.event(
        serde_json::json!({"type":"codex.rate_limits", "rate_limits":{
        "primary":{"usedPercent":100,"resetsAt":reset}}}),
    )
    .expect("snapshot");
    assert!(sink.quota);
    assert!(
        crate::routing::rate_limit::get_rate_limit_info(&AgentKind::Codex, None)
            .expect("hold")
            .recovery_at
            .is_some()
    );
}

fn run(fixture: &fixture::Fixture) -> Run<'_> {
    Run {
        start: std::time::Instant::now(),
        agent: &crate::agent::codex::CodexAgent,
        command: &fixture.command,
        task: &fixture.task,
        store: &fixture.store,
        log: &fixture.log,
        output: None,
        group: None,
        policy: crate::config::timeout_policy::TimeoutPolicy::default(),
        max_cost: None,
        control: None,
    }
}

#[test]
fn typed_usage_refusal_preserves_the_provider_reset_message() {
    let fixture = fixture::Fixture::new("complete");
    let run = run(&fixture);
    let mut sink = Sink::new(&run).expect("sink");
    let reset = chrono::Local::now().naive_local() + chrono::Duration::days(7);
    let message = dated_refusal(reset);
    let error = usage_error(&message);
    assert_eq!(
        crate::agent::codex::app_server::error_quota_message(&error),
        Some(message)
    );
    sink.event(error).expect("typed refusal");
    assert_reset(reset);
}

#[test]
fn snapshot_hold_survives_later_shorter_refusals_and_can_be_extended() {
    let fixture = fixture::Fixture::new("complete");
    let run = run(&fixture);
    let mut sink = Sink::new(&run).expect("sink");
    let reset = chrono::Utc::now() + chrono::Duration::days(7);
    sink.event(
        serde_json::json!({"type":"codex.rate_limits", "rate_limits":{
        "primary":{"usedPercent":100,"resetsAt":reset.timestamp()}}}),
    )
    .expect("snapshot");
    let expected = reset.with_timezone(&chrono::Local).naive_local();
    for message in [
        "You have hit your usage limit".to_string(),
        dated_refusal(expected - chrono::Duration::days(1)),
        "HTTP 429".to_string(),
    ] {
        sink.event(usage_error(&message)).expect("later refusal");
        assert_reset(expected);
    }
    let extended = expected + chrono::Duration::days(1);
    sink.event(usage_error(&dated_refusal(extended)))
        .expect("extended refusal");
    assert_reset(extended);
}

#[test]
fn typed_usage_refusal_without_message_uses_the_default_wording() {
    let event =
        serde_json::json!({"type":"error", "error":{"codexErrorInfo":"usageLimitExceeded"}});
    assert_eq!(
        crate::agent::codex::app_server::error_quota_message(&event),
        Some("You have hit your usage limit".to_string())
    );
}

fn usage_error(message: &str) -> Value {
    serde_json::json!({"type":"error", "error":{
        "codexErrorInfo":"usageLimitExceeded", "message":message}})
}

fn dated_refusal(reset: chrono::NaiveDateTime) -> String {
    let fixture = include_str!("../../../../tests/fixtures/rate-limit-codex");
    let message = fixture
        .lines()
        .find_map(|line| line.strip_prefix("message: "))
        .expect("message");
    let (_, date) = message
        .split_once("try again at ")
        .expect("provider reset phrase");
    message.replace(date, &crate::routing::rate_limit::format_recovery(reset))
}

fn assert_reset(reset: chrono::NaiveDateTime) {
    let info =
        crate::routing::rate_limit::get_rate_limit_info(&AgentKind::Codex, None).expect("hold");
    assert_eq!(
        info.recovery_at,
        Some(crate::routing::rate_limit::format_recovery(reset))
    );
}
