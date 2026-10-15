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
