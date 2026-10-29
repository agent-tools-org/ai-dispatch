// Supported dispatch ignores retired agent settings without losing sticky defaults.
// Deps: parent resolver tests, isolated aid home, Store.
use super::*;

#[test]
fn stale_removed_agent_config_does_not_block_resolving_supported_dispatch() {
    let dir = crate::support::test_env::TestHome::new();
    std::fs::write(
        dir.path().join("agent_config.toml"),
        "[gemini]\nmodel = 'old'\n[qwen]\ndisabled = true\n\
         [grok]\nmodel = 'grok-4.6'\n",
    )
    .expect("config");
    let _served = crate::agent::model_validation::MockServedModelsGuard::set(
        AgentKind::Grok, Some(vec!["grok-4.6".into()]),
    );
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "grok".into(),
        prompt: "Explain the architecture".into(),
        ..Default::default()
    };
    let setup = resolve_agent_setup(&store, &mut args, None).expect("supported dispatch");
    assert_eq!(setup.agent_kind, AgentKind::Grok);
    assert_eq!(setup.effective_model.as_deref(), Some("grok-4.6"));
}
