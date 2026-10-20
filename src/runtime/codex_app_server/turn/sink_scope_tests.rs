// App-server sink regression coverage for typed refusal and snapshot decisions.
// Exercises the real sink and Store without a subprocess or external provider.

use super::*;
#[path = "fixture.rs"]
mod fixture;

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
