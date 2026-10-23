// Grok refusals must come from the typed error message, with generic tokens bounded.
// Uses isolated quota markers and completion parsing without launching a CLI.

use super::*;

#[test]
fn generic_grok_429_cannot_borrow_another_providers_long_hold() {
    let home = tempfile::tempdir().expect("home");
    let _guard = crate::support::paths::AidHomeGuard::set(home.path());
    let event = json!({"type":"error", "message":
        "HTTP 429; premium request limit; trace 2099-01-02T03:04:05Z"});
    assert_eq!(
        parse_grok_completion(&event.to_string()).status,
        TaskStatus::Failed
    );
    let marker = std::fs::read_to_string(rate_limit::marker_path(&AgentKind::Grok, None))
        .expect("transient hold");
    assert_eq!(
        crate::routing::availability::stored_hold(&marker, &AgentKind::Grok),
        crate::routing::availability::StoredHold::Transient
    );
}

#[test]
fn grok_only_classifies_the_typed_error_message() {
    let home = tempfile::tempdir().expect("home");
    let _guard = crate::support::paths::AidHomeGuard::set(home.path());
    assert!(
        !crate::agent::stream_completion::record_quota_exhaustion(
            "usage balance exhausted",
            AgentKind::Grok,
            None,
            None
        )
        .recorded()
    );
    for event in [
        json!({"type":"result", "text":"usage balance exhausted", "stopReason":"end_turn"}),
        json!({"type":"error", "message":"socket closed", "text":"usage balance exhausted"}),
    ] {
        crate::agent::stream_completion::record_quota_exhaustion(
            &event.to_string(),
            AgentKind::Grok,
            None,
            None,
        );
        assert!(!rate_limit::marker_path(&AgentKind::Grok, None).exists());
    }
    let event = json!({"type":"error", "message":"Grok Build usage balance exhausted"});
    parse_grok_completion(&event.to_string());
    assert!(rate_limit::marker_path(&AgentKind::Grok, None).exists());
}
