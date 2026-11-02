// Incident regression: no-model agy dispatch must escape a spent Gemini family.
// Exercises dispatch setup with isolated marker/probe and served-model evidence.
use super::*;
use crate::agent::model_validation::MockServedModelsGuard;

fn assert_checked_family_command(setup: &AgentSetup) {
    assert_eq!(
        setup.effective_model.as_deref(),
        Some("claude-opus-5-5-medium")
    );
    let opts = agent::RunOpts {
        model: setup.effective_model.clone(),
        ..Default::default()
    };
    let command = setup.agent.build_command("say hi", &opts).expect("command");
    let args = crate::support::test_command::args(&command);
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--model", "claude-opus-5-5-medium"])
    );
}

#[test]
fn agy_no_model_escapes_spent_probe_and_untrusted_marker() {
    let home = crate::support::test_env::TestHome::new();
    let cache = tempfile::tempdir().expect("cache");
    let _cache = crate::routing::quota::live::CacheDirGuard::set(cache.path());
    let _served = MockServedModelsGuard::set(
        AgentKind::Antigravity,
        Some(vec![
            "gemini-4-argon-medium".into(),
            "claude-opus-5-5-medium".into(),
            "gpt-oss-120b-medium".into(),
        ]),
    );
    let now = chrono::Utc::now();
    std::fs::write(
        cache.path().join("agy.json"),
        serde_json::json!({
            "ok": true, "snapshot": {"provider": "agy", "fetched_at": now.to_rfc3339(),
            "windows": [{"label": "Gemini 4 Argon Weekly Limit", "used_percent": 100.0,
            "resets_at": (now + chrono::Duration::hours(36)).to_rfc3339()}]}
        })
        .to_string(),
    )
    .expect("probe");
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "agy".into(),
        prompt: "say hi".into(),
        ..Default::default()
    };
    let setup = resolve_agent_setup(&store, &mut args, None).expect("healthy family");
    assert_checked_family_command(&setup);
    std::fs::remove_file(cache.path().join("agy.json")).expect("remove probe");
    std::fs::write(
        crate::support::paths::aid_dir().join("rate-limit-agy--gemini-4-argon"),
        "source: untrusted\nmessage: Individual quota reached. Resets in 36h29m20s.\n",
    )
    .expect("marker");
    let setup = resolve_agent_setup(&store, &mut args, None).expect("marker family");
    assert_checked_family_command(&setup);
    drop(home);
}

#[test]
fn agy_family_explicit_pin_is_refused_and_all_spent_routes_are_blocked() {
    let _home = crate::support::test_env::TestHome::new();
    let cache = tempfile::tempdir().expect("cache");
    let _cache = crate::routing::quota::live::CacheDirGuard::set(cache.path());
    let _served = MockServedModelsGuard::set(
        AgentKind::Antigravity,
        Some(vec![
            "gemini-4-argon-high".into(),
            "claude-opus-5-5-high".into(),
        ]),
    );
    for family in ["gemini", "claude"] {
        rate_limit::mark_untrusted_for_model(
            &AgentKind::Antigravity,
            None,
            Some(if family == "gemini" {
                "gemini-4-argon-high"
            } else {
                "claude-opus-5-5-high"
            }),
            "Individual quota reached. Resets in 36h29m20s.",
        );
    }
    let marker = std::fs::read_to_string(
        crate::support::paths::aid_dir().join("rate-limit-agy--gemini-4-argon"),
    )
    .expect("marker");
    assert!(marker.contains("recovery_at:"));
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "agy".into(),
        prompt: "say hi".into(),
        model: Some("gemini-4-argon-high".into()),
        model_source: agent::model_validation::ModelSource::UserSupplied,
        ..Default::default()
    };
    assert!(resolve_agent_setup(&store, &mut args, None).is_err());
    assert_eq!(args.model.as_deref(), Some("gemini-4-argon-high"));
    assert!(agent::family_route::healthy_model(AgentKind::Antigravity, None, None).is_none());
    assert!(
        !crate::support::paths::aid_dir()
            .join("rate-limit-agy")
            .exists()
    );
}

#[test]
fn agy_family_caller_owned_keeps_exact_route_and_refuses_spent_family() {
    let home = crate::support::test_env::TestHome::new();
    let _cache = crate::routing::quota::live::CacheDirGuard::set(home.path());
    let _models = MockServedModelsGuard::set(
        AgentKind::Antigravity,
        Some(vec![
            "gemini-4-argon-high".into(),
            "claude-opus-5-5-high".into(),
        ]),
    );
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "agy".into(),
        model: Some("gemini-4-argon-high".into()),
        model_source: agent::model_validation::ModelSource::AidResolved,
        policy: Some("caller-owned".into()),
        ..Default::default()
    };
    let setup = resolve_agent_setup(&store, &mut args, None).expect("exact route");
    assert_eq!(setup.effective_model, args.model);
    rate_limit::mark_untrusted_for_model(
        &AgentKind::Antigravity,
        None,
        args.model.as_deref(),
        "Individual quota reached. Resets in 36h29m20s.",
    );
    assert!(resolve_agent_setup(&store, &mut args, None).is_err());
    assert_eq!(args.model.as_deref(), Some("gemini-4-argon-high"));
}

#[test]
fn agy_family_ordinary_user_pin_preserves_warned_substitution() {
    let home = crate::support::test_env::TestHome::new();
    let _cache = crate::routing::quota::live::CacheDirGuard::set(home.path());
    let _models = MockServedModelsGuard::set(
        AgentKind::Antigravity,
        Some(vec![
            "gemini-4-argon-high".into(),
            "claude-opus-5-5-high".into(),
        ]),
    );
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "agy".into(),
        prompt: "say hi".into(),
        model: Some("gemini-4-argon-high".into()),
        model_source: agent::model_validation::ModelSource::UserSupplied,
        ..Default::default()
    };
    let setup = resolve_agent_setup(&store, &mut args, None).expect("healthy explicit model");
    assert_eq!(setup.effective_model, args.model);
    rate_limit::mark_untrusted_for_model(
        &AgentKind::Antigravity,
        None,
        args.model.as_deref(),
        "Individual quota reached. Resets in 36h29m20s.",
    );
    let setup = resolve_agent_setup(&store, &mut args, None).expect("warned family replacement");
    assert_eq!(
        setup.effective_model.as_deref(),
        Some("claude-opus-5-5-high")
    );
    assert_eq!(args.model.as_deref(), Some("gemini-4-argon-high"));
    assert_eq!(
        args.model_source,
        agent::model_validation::ModelSource::AidResolved
    );
}

#[test]
fn agy_all_spent_walks_explicit_cascade() {
    let home = crate::support::test_env::TestHome::new();
    let _cache = crate::routing::quota::live::CacheDirGuard::set(home.path());
    let _served = MockServedModelsGuard::set(
        AgentKind::Antigravity,
        Some(vec!["gemini-4-argon-medium".into()]),
    );
    rate_limit::mark_untrusted_for_model(
        &AgentKind::Antigravity,
        None,
        Some("gemini-4-argon-medium"),
        "Individual quota reached. Resets in 36h.",
    );
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "agy".into(),
        cascade: vec!["opencode".into()],
        ..Default::default()
    };
    let setup = resolve_agent_setup(&store, &mut args, None).expect("cascade route");
    assert_eq!(setup.agent_kind, AgentKind::OpenCode);
}

#[test]
fn agy_default_effort_and_selfheal_do_not_select_high() {
    let home = crate::support::test_env::TestHome::new();
    let _cache = crate::routing::quota::live::CacheDirGuard::set(home.path());
    let _served = MockServedModelsGuard::set(
        AgentKind::Antigravity,
        Some(vec![
            "gemini-4-argon-high".into(),
            "gemini-4-argon-medium".into(),
            "gemini-3.8-flash-medium".into(),
        ]),
    );
    let store = Arc::new(Store::open_memory().expect("store"));
    for force_default_model in [false, true] {
        let mut args = RunArgs {
            agent_name: "agy".into(),
            force_default_model,
            ..Default::default()
        };
        let setup = resolve_agent_setup(&store, &mut args, None).expect("default");
        assert_eq!(
            setup.effective_model.as_deref(),
            Some("gemini-4-argon-medium")
        );
    }
}

#[test]
fn agy_metered_units_match_refusal_markers() {
    use agent::model_group::model_group;
    assert_eq!(
        model_group(AgentKind::Antigravity, Some("gemini-4-argon-high")),
        Some("gemini-4-argon")
    );
    assert_eq!(
        model_group(AgentKind::Antigravity, Some("gemini-3.8-flash-low")),
        Some("gemini")
    );
    for model in ["claude-opus-5-5-high", "gpt-oss-120b-medium"] {
        assert_eq!(
            model_group(AgentKind::Antigravity, Some(model)),
            Some("claude-gpt")
        );
    }
}

#[test]
fn agy_shared_refusal_unit_is_excluded_without_a_trusted_reset() {
    let home = crate::support::test_env::TestHome::new();
    let _cache = crate::routing::quota::live::CacheDirGuard::set(home.path());
    let _models = MockServedModelsGuard::set(AgentKind::Antigravity, Some(vec![
        "claude-opus-5-5-medium".into(), "gpt-oss-120b-medium".into(),
        "gemini-4-argon-medium".into(),
    ]));
    let model = agent::family_route::continuation_model(
        AgentKind::Antigravity, None, Some("claude-opus-5-5-medium"),
    );
    assert_eq!(model.as_deref(), Some("gemini-4-argon-medium"));
    rate_limit::mark_untrusted_for_model(&AgentKind::Antigravity, None,
        Some("claude-opus-5-5-medium"), "Individual quota reached. Resets in 36h.");
    assert!(crate::routing::availability::family_is_spent(AgentKind::Antigravity, None, "claude-gpt"));
}

#[test]
fn agy_budget_replacement_prefers_cheapest_line_at_requested_effort() {
    let home = crate::support::test_env::TestHome::new();
    let _cache = crate::routing::quota::live::CacheDirGuard::set(home.path());
    let _models = MockServedModelsGuard::set(AgentKind::Antigravity, Some(vec![
        "gemini-4-argon-medium".into(), "claude-opus-5-5-medium".into(),
        "claude-sonnet-5-5-medium".into(), "gemini-3.8-flash-medium".into(),
    ]));
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs { agent_name: "agy".into(), budget: true, ..Default::default() };
    let setup = resolve_agent_setup(&store, &mut args, None).expect("budget route");
    assert_eq!(setup.effective_model.as_deref(), Some("gemini-3.8-flash-medium"));
    rate_limit::mark_untrusted_for_model(&AgentKind::Antigravity, None,
        Some("gemini-3.8-flash-medium"), "Individual quota reached. Resets in 36h.");
    let setup = resolve_agent_setup(&store, &mut args, None).expect("budget replacement");
    assert_eq!(setup.effective_model.as_deref(), Some("claude-sonnet-5-5-medium"));
}
