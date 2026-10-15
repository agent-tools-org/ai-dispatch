// Dispatch and cascade identity regressions for API accounts.
// Uses a temporary config and the real resolver with an in-memory store.
use super::*;
use crate::support::paths::{self, AidHomeGuard};

#[test]
fn named_route_resolves_and_cascade_preserves_its_identity() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    std::fs::write(
        paths::config_path(),
        concat!(
            "[[api_route]]\nid = 'claude-api-ming'\nagent = 'claude'\n",
            "keychain_service = 'test'\nmonthly_budget_usd = 200\n",
            "[[api_route]]\nid = 'claude-api-meet'\nagent = 'claude'\n",
            "keychain_service = 'test'\nmonthly_budget_usd = 200\n",
        ),
    )
    .unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let mut args = RunArgs {
        agent_name: "claude-api-meet".into(),
        ..Default::default()
    };
    let setup = resolve_agent_setup(&store, &mut args, None).unwrap();
    assert_eq!(setup.agent_kind, AgentKind::Custom);
    assert_eq!(setup.custom_agent_name.as_deref(), Some("claude-api-meet"));
    crate::routing::api_budget::hold("claude-api-meet", "test hold", false).unwrap();
    args.cascade = vec!["claude-api-ming".into()];
    let setup = resolve_agent_setup(&store, &mut args, None).unwrap();
    assert_eq!(setup.agent_display_name, "claude-api-ming");
    assert_eq!(setup.agent.rate_limit_name(), Some("claude-api-ming"));
    assert_eq!(args.agent_name, "claude-api-ming");
}

#[test]
fn api_container_is_rejected_before_launch_without_holding_route() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    std::fs::write(
        paths::config_path(),
        concat!(
            "[[api_route]]\nid = 'claude-api-ming'\nagent = 'claude'\n",
            "keychain_service = 'test'\nmonthly_budget_usd = 200\n",
        ),
    )
    .unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let mut args = RunArgs {
        agent_name: "claude-api-ming".into(),
        container: Some("image".into()),
        ..Default::default()
    };
    let error = match resolve_agent_setup(&store, &mut args, None) {
        Ok(_) => panic!("API route container must be rejected"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("route claude-api-ming does not support --container"));
    assert!(!paths::aid_dir().join("rate-limit-claude-api-ming").exists());
}

#[test]
fn ordinary_dispatch_survives_invalid_unrelated_api_route() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    std::fs::write(
        paths::config_path(),
        concat!(
            "[[api_route]]\nid = 'claude-api-bad'\nagent = 'unsupported'\n",
            "keychain_service = 'test'\nmonthly_budget_usd = 200\n"
        ),
    )
    .unwrap();
    let agents = paths::aid_dir().join("agents");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::write(
        agents.join("other.toml"),
        "[agent]\nid = 'other'\ndisplay_name = 'Other'\ncommand = 'other'",
    )
    .unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    for name in ["claude", "codex", "other"] {
        let mut args = RunArgs {
            agent_name: name.into(),
            ..Default::default()
        };
        let setup = resolve_agent_setup(&store, &mut args, None).unwrap();
        assert_eq!(setup.agent_display_name, name);
    }
    let mut args = RunArgs {
        agent_name: "claude-api-bad".into(),
        ..Default::default()
    };
    let error = match resolve_agent_setup(&store, &mut args, None) {
        Ok(_) => panic!("invalid route must fail"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("only agent = \"claude\" is supported"));
}
