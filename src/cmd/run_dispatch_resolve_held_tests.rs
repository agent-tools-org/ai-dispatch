// Held-route dispatch regressions for model and provider quota markers.
// Exports: resolver tests for Cursor and OpenCode held routes.
// Deps: run resolver, rate_limit, Store, AgentKind.
use super::*;
use super::super::held::{
    held_substitution_detail, held_substitution_metadata, model_class_preserved,
};

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

/// `--urgency background` keeps a held ungrouped agent. The for_model facade
/// now answers agent-level holds; the resolve second gate must not undo that.
#[test]
fn background_urgency_keeps_held_ungrouped_agent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _guard = AidHomeGuard::set(dir.path());
    crate::rate_limit::mark_rate_limited(
        &AgentKind::Grok,
        None,
        "API error (status 402 Payment Required): Grok Build usage balance exhausted",
    );
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "grok".to_string(),
        prompt: "Add unit tests".to_string(),
        declared_urgency: Some(crate::types::TaskUrgency::Background),
        cascade: vec![],
        ..Default::default()
    };

    let setup = resolve_agent_setup(&store, &mut args)
        .expect("background must keep the held grok agent");
    assert_eq!(setup.agent_kind, AgentKind::Grok);
    assert_eq!(args.agent_name, "grok");
    assert!(setup.substituted_from.is_none());
}

#[test]
fn dry_run_milestone_says_would_dispatch() {
    let detail = held_substitution_detail("grok", "until dated grok snapshot", "claude", true);
    assert!(detail.contains("would dispatch to claude"), "{detail}");
    assert!(!detail.contains("dispatching to"), "{detail}");
}

#[test]
fn substitution_metadata_names_both_routes() {
    let meta = held_substitution_metadata(
        "grok",
        "claude",
        None,
        None,
        "until dated grok snapshot",
        "windowed",
        true,
    );
    assert_eq!(meta["kind"], "quota_substitution");
    assert_eq!(meta["from_agent"], "grok");
    assert_eq!(meta["to_agent"], "claude");
    assert_eq!(meta["wall"], "windowed");
    assert_eq!(meta["model_class_preserved"], false);
    assert_eq!(meta["dry_run"], true);
    assert!(!model_class_preserved("grok", "claude", None, None));
}
