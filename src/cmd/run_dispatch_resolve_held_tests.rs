// Held-route dispatch regressions for model and provider quota markers.
// Exports: resolver tests for Cursor and OpenCode held routes.
// Deps: run resolver, rate_limit, Store, AgentKind.
use super::*;

#[test]
fn held_opencode_provider_switches_before_dispatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _guard = AidHomeGuard::set(dir.path());
    crate::rate_limit::mark_group_rate_limited(
        &AgentKind::OpenCode,
        None,
        "nvidia",
        "Insufficient balance.",
    );
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "opencode".to_string(),
        prompt: "Add unit tests".to_string(),
        model: Some("nvidia/llama-4-maverick".to_string()),
        cascade: vec!["codex".to_string()],
        ..Default::default()
    };

    let setup = resolve_agent_setup(&store, &mut args).expect("should switch to fallback");

    assert_eq!(setup.agent_kind, AgentKind::Codex);
    assert_eq!(args.agent_name, "codex");
    assert!(setup.substituted_from.is_some());
}

#[test]
fn held_cursor_premium_switches_to_auto_without_changing_agent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _guard = AidHomeGuard::set(dir.path());
    crate::rate_limit::mark_group_rate_limited(
        &AgentKind::Cursor,
        None,
        "premium",
        "You're out of usage. Switch to Auto.",
    );
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "cursor".to_string(),
        prompt: "Add unit tests".to_string(),
        model: Some("composer-2.5".to_string()),
        cascade: vec!["codex".to_string()],
        ..Default::default()
    };

    let setup = resolve_agent_setup(&store, &mut args).expect("should switch model in cursor");

    assert_eq!(setup.agent_kind, AgentKind::Cursor);
    assert_eq!(setup.effective_model.as_deref(), Some("auto"));
    assert_eq!(args.agent_name, "cursor");
    assert!(setup.substituted_from.is_none());
}
