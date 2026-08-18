// Held-route model-pin survival regressions for `aid run` dispatch.
// Exports: pin survival across group holds, served-list misses, and the
// broader agent-config/budget model case that falls out of switch_agent
// clearing args.model on a substituted route.
// Deps: run resolver, rate_limit, model validation mock, agent_config, Store.
use super::*;

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

/// The pin from `substituted_fallback_is_pinned_to_a_group_that_can_serve` is
/// `ModelSource::AidResolved`, and `validate_model_for_agent` drops an
/// aid-resolved model that is not in the served list — setting
/// `effective_model = None`, after which agy runs its own default, a gemini
/// model: the exact family the substitution just avoided. The served-list
/// snapshot can lag the catalog (a partial probe, or a cache written before the
/// claude family was discovered), so this must reproduce through the
/// substitution path, not by calling the validator directly.
#[test]
fn substituted_aid_resolved_pin_survives_served_list_miss() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _guard = AidHomeGuard::set(dir.path());
    crate::rate_limit::mark_group_rate_limited(
        &AgentKind::OpenCode,
        None,
        "opencode",
        "Insufficient balance.",
    );
    crate::rate_limit::mark_group_rate_limited(
        &AgentKind::Antigravity,
        None,
        "gemini",
        "Individual quota reached. Please upgrade your subscription to increase your limits. Resets in 59m21s.",
    );
    // agy's served-list snapshot predates the claude family — the pinned
    // claude model is absent. This is the cache shape that trips the drop.
    let _served = crate::agent::model_validation::MockServedModelsGuard::set(
        AgentKind::Antigravity,
        Some(vec![
            "gemini-3.1-pro-high".to_string(),
            "gemini-3.6-flash-high".to_string(),
            "gemini-3.6-flash-low".to_string(),
        ]),
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
    let model = setup
        .effective_model
        .expect("aid-resolved pin must survive a served-list miss on a substituted route");
    assert_ne!(
        crate::agent::model_group::model_group(AgentKind::Antigravity, Some(&model)),
        Some("gemini"),
        "the exhausted gemini family must not be dispatched"
    );
    assert!(setup.substituted_from.is_some());
}

/// The `keep_aid_resolved_pin` guard is deliberately wider than the family pin
/// from `healthy_model_for`. Because `switch_agent` clears `args.model`, every
/// `effective_model` on a substituted route is `ModelSource::AidResolved` —
/// including a model read from `~/.aid/agent_config.toml` for the fallback
/// agent. That model survives a served-list miss too: the miss is cache lag,
/// not proof the CLI will reject the model, and the alternative is the
/// fallback's own default, which for a family-metered agent can re-enter the
/// exhausted group. This pins the breadth as a decision with a test behind it,
/// not a side effect of `switch_agent` clearing `args.model`. Contrast
/// `aid_selected_default_is_persisted_as_aid_resolved` in model_source_tests,
/// where the same config model IS dropped because the route was not
/// substituted.
#[test]
fn substituted_route_keeps_agent_config_model_despite_served_list_miss() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _guard = AidHomeGuard::set(dir.path());
    // grok is agent-level held — the agent-level substitution trigger.
    crate::rate_limit::mark_rate_limited(
        &AgentKind::Grok,
        None,
        &format!(
            "Individual quota reached. Please upgrade your subscription. try again at {}.",
            crate::rate_limit::test_future_recovery_time()
        ),
    );
    // The fallback (codex) has a configured default model that is NOT in its
    // served-list snapshot — the cache shape that drops an aid-resolved model
    // on a non-substituted route.
    crate::agent_config::save_agent_default_model("codex", Some("codex-stale-config-model"))
        .expect("save codex default");
    let _served = crate::agent::model_validation::MockServedModelsGuard::set(
        AgentKind::Codex,
        Some(vec!["gpt-5.4".to_string()]),
    );
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "grok".to_string(),
        prompt: "Add unit tests".to_string(),
        cascade: vec!["codex".to_string()],
        ..Default::default()
    };

    let setup = resolve_agent_setup(&store, &mut args).expect("should substitute grok to codex");

    assert_eq!(setup.agent_kind, AgentKind::Codex);
    assert_eq!(
        setup.effective_model.as_deref(),
        Some("codex-stale-config-model"),
        "an agent-config model on a substituted route must survive a served-list miss"
    );
    assert!(setup.substituted_from.is_some());
}
