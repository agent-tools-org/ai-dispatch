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

/// t-44b30780 chain: opencode was held on its `opencode` provider group, so
/// `skip_held_to_fallback` substituted agy — whose gemini group marker
/// (`rate-limit-agy--gemini`) had existed since 10:40 — and agy died in 10
/// seconds with "Individual quota reached". When every group a fallback can
/// draw on is held, it is as unusable as an agent-level hold and must be
/// skipped.
#[test]
fn group_held_fallback_is_skipped_when_cascade_steps_over_held_route() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _guard = AidHomeGuard::set(dir.path());
    crate::rate_limit::mark_group_rate_limited(
        &AgentKind::OpenCode,
        None,
        "opencode",
        "Insufficient balance.",
    );
    // agy's gemini AND claude allowances are exhausted — nothing it can serve.
    crate::rate_limit::mark_group_rate_limited(
        &AgentKind::Antigravity,
        None,
        "gemini",
        "Individual quota reached. Please upgrade your subscription to increase your limits. Resets in 59m21s.",
    );
    crate::rate_limit::mark_group_rate_limited(
        &AgentKind::Antigravity,
        None,
        "claude",
        "Individual quota reached. Please upgrade your subscription to increase your limits. Resets in 59m21s.",
    );
    crate::rate_limit::mark_group_rate_limited(
        &AgentKind::Antigravity,
        None,
        "gpt-oss",
        "Individual quota reached. Please upgrade your subscription to increase your limits. Resets in 59m21s.",
    );
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "opencode".to_string(),
        prompt: "Add unit tests".to_string(),
        model: Some("opencode/deepseek-v4-pro".to_string()),
        cascade: vec!["agy".to_string(), "codex".to_string()],
        ..Default::default()
    };

    let setup = resolve_agent_setup(&store, &mut args).expect("should skip held agy and reach codex");

    assert_eq!(
        setup.agent_kind,
        AgentKind::Codex,
        "agy is group-held on every family it can serve and must not be the fallback"
    );
    assert_eq!(args.agent_name, "codex");
    assert!(setup.substituted_from.is_some());
    let (original, _) = setup.substituted_from.as_ref().expect("substituted");
    assert_eq!(original, "opencode");
}

/// The fallback that *is* picked must not run a model from the exhausted group.
/// With only gemini held, agy's claude allowance still serves — but its default
/// model is a gemini one, so `effective_model = None` hands the task right back
/// to the spent family. Substitution must pin the fallback to a healthy group.
#[test]
fn substituted_fallback_is_pinned_to_a_group_that_can_serve() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _guard = AidHomeGuard::set(dir.path());
    // opencode is held on its own provider group — the substitution trigger.
    crate::rate_limit::mark_group_rate_limited(
        &AgentKind::OpenCode,
        None,
        "opencode",
        "Insufficient balance.",
    );
    // agy's gemini allowance is exhausted; claude still serves.
    crate::rate_limit::mark_group_rate_limited(
        &AgentKind::Antigravity,
        None,
        "gemini",
        "Individual quota reached. Please upgrade your subscription to increase your limits. Resets in 59m21s.",
    );
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "opencode".to_string(),
        prompt: "Add unit tests".to_string(),
        model: Some("opencode/deepseek-v4-pro".to_string()),
        cascade: vec!["agy".to_string()],
        ..Default::default()
    };

    let setup = resolve_agent_setup(&store, &mut args).expect("should substitute opencode to agy");

    assert_eq!(setup.agent_kind, AgentKind::Antigravity);
    let model = setup.effective_model.expect("substitution must pin a healthy model");
    assert_ne!(
        crate::agent::model_group::model_group(AgentKind::Antigravity, Some(&model)),
        Some("gemini"),
        "the exhausted gemini family must not be dispatched"
    );
    assert!(setup.substituted_from.is_some());
}

/// Second-question reproduction (11:07 dry-run): the fallback candidate has an
/// agent-level marker with a future recovery time (`rate-limit-agy` with
/// `recovery_at: Aug 18, 2026 02:41 PM` style). Even pre-fix, the candidate
/// gate read agent-level markers, so this must hold agy and skip it.
#[test]
fn agent_level_held_fallback_is_skipped_when_cascade_steps_over_held_route() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _guard = AidHomeGuard::set(dir.path());
    crate::rate_limit::mark_group_rate_limited(
        &AgentKind::OpenCode,
        None,
        "opencode",
        "Insufficient balance.",
    );
    // agy's agent-level marker names a future recovery — the exact 11:07 shape.
    crate::rate_limit::mark_rate_limited(
        &AgentKind::Antigravity,
        None,
        &format!("Individual quota reached. Please upgrade your subscription. try again at {}.", crate::rate_limit::test_future_recovery_time()),
    );
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "opencode".to_string(),
        prompt: "Add unit tests".to_string(),
        model: Some("opencode/deepseek-v4-pro".to_string()),
        cascade: vec!["agy".to_string(), "codex".to_string()],
        ..Default::default()
    };

    let setup = resolve_agent_setup(&store, &mut args).expect("should skip held agy and reach codex");

    assert_eq!(
        setup.agent_kind,
        AgentKind::Codex,
        "agy is agent-level held and must not be the fallback"
    );
    assert_eq!(args.agent_name, "codex");
    assert!(setup.substituted_from.is_some());
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
