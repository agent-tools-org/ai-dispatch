// Qwen routing and refusal classification regressions.
// Keeps Google refusals out of Qwen and bounds untyped provider holds.
use super::*;
use crate::agent::Agent;
use crate::routing::{availability, rate_limit};
use crate::support::paths::AidHomeGuard;

#[test]
fn google_only_refusals_do_not_hold_qwen() {
    let home = tempfile::tempdir().expect("home");
    let _home = AidHomeGuard::set(home.path());
    for message in [
        "IneligibleTierError",
        "migrate to antigravity",
        "ineligibletier",
    ] {
        let event = serde_json::json!({"type":"error", "message":message}).to_string();
        let parsed = QwenAgent
            .parse_event(&TaskId("t-qwen".into()), &event)
            .expect("error event");
        assert_eq!(parsed.event_kind, EventKind::Error);
        assert!(!rate_limit::marker_path(&AgentKind::Qwen, None).exists());
        assert!(
            !super::super::stream_completion::record_quota_exhaustion(
                &event,
                AgentKind::Qwen,
                None,
                None,
            )
            .recorded()
        );
    }
}

#[test]
fn qwen_429_and_generic_rate_limits_only_create_transient_holds() {
    let home = tempfile::tempdir().expect("home");
    let _home = AidHomeGuard::set(home.path());
    for message in [
        "429 Too Many Requests",
        "rate limit reached",
        "429 try again at Dec 31st, 2099 10:12 AM",
    ] {
        let event = serde_json::json!({"type":"error", "message":message}).to_string();
        QwenAgent
            .parse_event(&TaskId("t-qwen".into()), &event)
            .expect("error event");
        let path = rate_limit::marker_path(&AgentKind::Qwen, None);
        let marker = std::fs::read_to_string(path).expect("transient marker");
        assert_eq!(
            availability::stored_hold(&marker, &AgentKind::Qwen),
            availability::StoredHold::Transient
        );
        assert!(rate_limit::marker_field(&marker, "recovery_at: ").is_none());
        assert!(!marker.contains("hold: manual"));
    }
}

#[test]
fn qwen_terminal_refusal_remains_an_untrusted_excerpt() {
    let home = tempfile::tempdir().expect("home");
    let _home = AidHomeGuard::set(home.path());
    let event = serde_json::json!({"type":"result", "is_error":false,
        "result":"Quota exhausted: Your token-plan 1-week quota has been exhausted. The quota will reset at 12-31 10:12:00 UTC."}).to_string();
    let outcome = super::super::stream_completion::record_quota_exhaustion(
        &event,
        AgentKind::Qwen,
        None,
        None,
    );
    assert!(outcome.recorded());
    assert!(!outcome.should_fail());
    let marker = std::fs::read_to_string(rate_limit::marker_path(&AgentKind::Qwen, None))
        .expect("excerpt marker");
    assert_eq!(
        availability::stored_hold(&marker, &AgentKind::Qwen),
        availability::StoredHold::Transient
    );
    assert!(marker.contains("source: untrusted"));
}

#[test]
fn qwen_token_plan_error_is_provider_attributed() {
    let home = tempfile::tempdir().expect("home");
    let _home = AidHomeGuard::set(home.path());
    let event = serde_json::json!({"type":"error",
        "message":"Quota exhausted: Your token-plan 5-hour quota has been exhausted."})
    .to_string();
    QwenAgent
        .parse_event(&TaskId("t-qwen".into()), &event)
        .expect("error event");
    assert!(rate_limit::is_rate_limited(&AgentKind::Qwen, None));
    let info = rate_limit::get_rate_limit_info(&AgentKind::Qwen, None).expect("hold");
    assert!(!info.needs_human);
    assert!(info.recovery_at.is_some());
}

#[test]
fn qwen_remains_a_builtin_dispatch_and_configuration_route() {
    let home = tempfile::tempdir().expect("home");
    let _home = AidHomeGuard::set(home.path());
    assert_eq!(AgentKind::parse_str("QWEN"), Some(AgentKind::Qwen));
    assert!(AgentKind::ALL_BUILTIN.contains(&AgentKind::Qwen));
    assert_eq!(
        crate::agent::get_agent(AgentKind::Qwen).kind(),
        AgentKind::Qwen
    );
    super::super::removed::reject("qwen").expect("supported name");
    crate::config::agent::save_agent_default_model("qwen", Some("coder-model")).expect("config");
    assert_eq!(
        crate::config::agent::get_default_model("qwen").as_deref(),
        Some("coder-model")
    );
    super::super::removed::validate_run(&crate::cmd::run::RunArgs {
        agent_name: "qwen".into(),
        judge: Some("qwen".into()),
        peer_review: Some("qwen".into()),
        cascade: vec!["qwen".into()],
        ..Default::default()
    })
    .expect("supported dispatch routes");
    crate::config::team::parse_team(
        "[team]\nid = 'qwen-team'\ndisplay_name = 'Qwen'\npreferred_agents = ['qwen']\n",
    )
    .expect("supported team route");
    let path = home.path().join("batch.toml");
    std::fs::write(&path, "[[tasks]]\nagent = 'qwen'\nprompt = 'Review code'\n").expect("batch");
    crate::batch::parse_batch_file(&path).expect("supported batch route");
}
