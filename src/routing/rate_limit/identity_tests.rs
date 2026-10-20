// Marker ownership, custom signature identities, and explicit reset round trips.
// Exercises the public routing facades with isolated marker and agent-config files.

use super::*;
use crate::routing::{availability, rate_limit};
use crate::support::paths::{self, AidHomeGuard};
use crate::types::{AgentKind, TaskId};

#[test]
fn custom_balance_hold_is_read_under_its_own_id() {
    let home = tempfile::tempdir().expect("home");
    let _guard = AidHomeGuard::set(home.path());
    rate_limit::mark_rate_limited(&AgentKind::Custom, Some("auditor"), "Insufficient balance");
    assert!(rate_limit::is_rate_limited(
        &AgentKind::Custom,
        Some("auditor")
    ));
    assert!(!rate_limit::is_rate_limited(
        &AgentKind::Custom,
        Some("reviewer")
    ));
    assert!(!rate_limit::is_rate_limited(&AgentKind::OpenCode, None));
    let hint = rate_limit::dispatch_blocking_hold(&AgentKind::Custom, Some("auditor"))
        .expect("blocking hint");
    assert!(hint.contains("clear-limit auditor"));
}

#[test]
fn explicit_reset_markers_round_trip_without_changing_the_stated_date() {
    let home = tempfile::tempdir().expect("home");
    let _guard = AidHomeGuard::set(home.path());
    let stated = rate_limit::test_future_recovery_time();
    for (kind, name) in [
        (AgentKind::Custom, Some("auditor")),
        (AgentKind::MiMoCode, None),
        (AgentKind::Codex, None),
    ] {
        rate_limit::mark_rate_limited(&kind, name, &format!("try again at {stated}."));
        assert!(rate_limit::is_rate_limited(&kind, name));
        let info = rate_limit::get_rate_limit_info(&kind, name).expect("marker");
        assert_eq!(info.recovery_at.as_deref(), Some(stated.as_str()));
    }
}

#[test]
fn overlay_signature_identity_survives_readback_without_a_registry_entry() {
    let home = tempfile::tempdir().expect("home");
    let _guard = AidHomeGuard::set(home.path());
    let event = serde_json::json!({"type":"error", "error":{"data":{
        "message":"You have hit your usage limit"}}});
    crate::agent::opencode::parse_json_event(
        AgentKind::Codex,
        AgentKind::Custom,
        Some("reviewer"),
        &TaskId("t-overlay".into()),
        &event,
        chrono::Local::now(),
    )
    .expect("provider error");
    let content = std::fs::read_to_string(rate_limit::marker_path(
        &AgentKind::Custom,
        Some("reviewer"),
    ))
    .expect("owned marker");
    assert_eq!(
        stored_signature_agent(&content, AgentKind::Custom),
        AgentKind::Codex
    );
    assert!(rate_limit::is_rate_limited(
        &AgentKind::Custom,
        Some("reviewer")
    ));
    assert!(!rate_limit::is_rate_limited(&AgentKind::Codex, None));
}

#[test]
fn configured_custom_signature_is_used_by_direct_marker_writes() {
    let home = tempfile::tempdir().expect("home");
    let _guard = AidHomeGuard::set(home.path());
    let agents = paths::aid_dir().join("agents");
    std::fs::create_dir_all(&agents).expect("agents");
    std::fs::write(
        agents.join("spy.toml"),
        concat!(
            "[agent]\nid='spy'\ndisplay_name='Spy'\ncommand='/bin/sh'\n",
            "rate_limit_kind='oz'\n",
        ),
    )
    .expect("config");
    rate_limit::mark_rate_limited(
        &AgentKind::Custom,
        Some("spy"),
        "Your credentials are invalid",
    );
    assert!(rate_limit::is_rate_limited(&AgentKind::Custom, Some("spy")));
    let content = std::fs::read_to_string(rate_limit::marker_path(&AgentKind::Custom, Some("spy")))
        .expect("marker");
    assert_eq!(
        stored_signature_agent(&content, AgentKind::Custom),
        AgentKind::Oz
    );
    std::fs::remove_file(agents.join("spy.toml")).expect("remove config");
    assert!(rate_limit::is_rate_limited(&AgentKind::Custom, Some("spy")));
}

#[test]
fn stored_custom_identity_cannot_widen_builtin_or_untrusted_signatures() {
    use availability::StoredHold;
    let balance = "signature_agent: opencode\nmessage: Insufficient balance\n";
    assert_eq!(
        availability::stored_hold(balance, &AgentKind::Custom),
        StoredHold::NeedsHuman
    );
    assert_eq!(
        availability::stored_hold(balance, &AgentKind::Codex),
        StoredHold::Transient
    );
    let unrelated = "signature_agent: codex\nmessage: Insufficient balance\n";
    assert_eq!(
        availability::stored_hold(unrelated, &AgentKind::Custom),
        StoredHold::Transient
    );
    let untrusted = format!("source: untrusted\n{balance}");
    assert_eq!(
        availability::stored_hold(&untrusted, &AgentKind::Custom),
        StoredHold::Transient
    );
}

#[test]
fn provider_group_holds_keep_the_engine_signature_and_provider_scope() {
    let home = tempfile::tempdir().expect("home");
    let _guard = AidHomeGuard::set(home.path());
    rate_limit::mark_rate_limited_for_model(
        &AgentKind::OpenCode,
        None,
        Some("nvidia/llama-4-maverick"),
        "Insufficient balance",
    );
    assert!(rate_limit::is_group_rate_limited(
        &AgentKind::OpenCode,
        None,
        "nvidia"
    ));
    assert!(!rate_limit::is_group_rate_limited(
        &AgentKind::OpenCode,
        None,
        "opencode"
    ));
    assert!(!rate_limit::is_rate_limited(&AgentKind::OpenCode, None));
}
