// Incident regression: no-model agy dispatch must escape a spent Gemini family.
// Exercises dispatch setup with isolated marker/probe and served-model evidence.
use super::*;
use crate::agent::model_validation::MockServedModelsGuard;

#[test]
fn agy_no_model_escapes_spent_probe_and_untrusted_marker() {
    let home = crate::support::test_env::TestHome::new();
    let cache = tempfile::tempdir().expect("cache");
    let _cache = crate::routing::quota::live::CacheDirGuard::set(cache.path());
    let _served = MockServedModelsGuard::set(
        AgentKind::Antigravity,
        Some(vec![
            "gemini-4-argon-high".into(),
            "claude-opus-5-5-high".into(),
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
    assert_eq!(
        setup.effective_model.as_deref(),
        Some("claude-opus-5-5-high")
    );
    std::fs::remove_file(cache.path().join("agy.json")).expect("remove probe");
    std::fs::write(
        crate::support::paths::aid_dir().join("rate-limit-agy--gemini"),
        "source: untrusted\nmessage: Individual quota reached. Resets in 36h29m20s.\n",
    )
    .expect("marker");
    let setup = resolve_agent_setup(&store, &mut args, None).expect("marker family");
    assert_eq!(
        setup.effective_model.as_deref(),
        Some("claude-opus-5-5-high")
    );
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
    let marker =
        std::fs::read_to_string(crate::support::paths::aid_dir().join("rate-limit-agy--gemini"))
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
