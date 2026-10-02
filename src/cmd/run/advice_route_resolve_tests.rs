// Exact advice models through resolution, held substitution and budget pressure.
// Uses the parent routing fixtures and resolver/model validation mocks.
use super::*;

#[test]
fn selected_defaults_are_exact_through_budget_pressure_and_cross_agent_reset() {
    let (dir, _home, _cache) = isolated();
    let _fleet = DetectAgentsGuard::set(vec![AgentKind::Codex]);
    let store = Arc::new(Store::open_memory().expect("store"));
    let codex = dir.path().join("codex-home");
    std::fs::create_dir(&codex).expect("CLI home");
    crate::agent::codex::cli_config::set_test_codex_home(Some(codex.clone()));
    let _served =
        crate::agent::model_validation::MockServedModelsGuard::set(AgentKind::Codex, None);
    for model in [Some("gpt-6-sol"), None] {
        if let Some(model) = model {
            std::fs::write(codex.join("config.toml"), format!("model = '{model}'\n"))
                .expect("CLI model");
        } else {
            std::fs::remove_file(codex.join("config.toml")).expect("clear CLI model");
        }
        let mut child = RunArgs {
            budget: true,
            model: Some("old-model".into()),
            session_id: Some("old-session".into()),
            ..args()
        };
        let selected = automatic_candidate(Some(&store), &child).expect("peer");
        assert_eq!(selected.model.as_deref(), model);
        apply_candidate(&mut child, &selected);
        assert!(child.session_id.is_none());
        assert_eq!(child.model_source, ModelSource::Advised);
        assert_eq!(child.force_default_model, model.is_none());
        std::fs::write(
            dir.path().join("config.toml"),
            "[selection]\nbudget_mode = true\n",
        )
        .expect("pressure");
        let setup =
            crate::cmd::run::run_dispatch_resolve::resolve_agent_setup(&store, &mut child, None)
                .expect("route");
        assert_eq!(setup.effective_model.as_deref(), model);
        assert_eq!(child.model_source, ModelSource::Advised);
        let saved = RunArgs::from_dispatch_args_json(&child.dispatch_args_json().expect("save")).expect("restore");
        assert_eq!(saved.model_source, ModelSource::Advised);
        std::fs::remove_file(dir.path().join("config.toml")).expect("clear pressure");
    }
}

#[test]
fn automatic_preserves_candidate_in_both_held_resolution_phases() {
    let (dir, _home, _cache) = isolated();
    let _fleet = DetectAgentsGuard::set(vec![AgentKind::Codex, AgentKind::OpenCode]);
    crate::agent_config::save_agent_default_model("codex", Some("gpt-6-sol")).expect("model");
    let _served =
        crate::agent::model_validation::MockServedModelsGuard::set(AgentKind::Codex, None);
    let store = Arc::new(Store::open_memory().expect("store"));
    for route in ["opencode", "opencode--nvidia"] {
        hold(dir.path(), route);
        let mut child = RunArgs {
            agent_name: "opencode".into(),
            model: Some("nvidia/old".into()),
            ..args()
        };
        let selected = expected(&store, &child).expect("peer");
        let setup =
            crate::cmd::run::run_dispatch_resolve::resolve_agent_setup(&store, &mut child, None)
                .expect("substitute");
        assert_eq!(child.agent_name, selected.agent);
        assert_eq!(setup.effective_model, selected.model);
        assert!(child.model_source == ModelSource::Advised && setup.substituted_from.is_some());
        std::fs::remove_file(dir.path().join(format!("rate-limit-{route}"))).expect("clear");
    }
}

#[test]
fn advice_rejects_unserved_model_or_new_hold_without_dropping_or_family_switching() {
    let (dir, _home, _cache) = isolated();
    let _fleet = DetectAgentsGuard::set(vec![AgentKind::Droid]);
    crate::agent_config::save_agent_default_model("droid", Some("gpt-5.3-codex")).expect("model");
    let store = Arc::new(Store::open_memory().expect("store"));
    let selected = automatic_candidate(Some(&store), &args()).expect("peer");
    let mut child = args();
    apply_candidate(&mut child, &selected);
    let _served = crate::agent::model_validation::MockServedModelsGuard::set(
        AgentKind::Droid,
        Some(vec!["glm-5.2".into()]),
    );
    let error =
        crate::cmd::run::run_dispatch_resolve::resolve_agent_setup(&store, &mut child, None)
            .err()
            .expect("unserved");
    assert!(error.to_string().contains("refusing a different default"));
    hold(dir.path(), "droid--standard");
    let error =
        crate::cmd::run::run_dispatch_resolve::resolve_agent_setup(&store, &mut child, None)
            .err()
            .expect("held");
    assert!(error.to_string().contains("is held"));
    assert_eq!(child.model, selected.model);
}
