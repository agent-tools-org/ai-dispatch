// Best-of racer defaults pass through the shared family dispatch invariant.
// Uses advice candidate construction with isolated served-model and quota evidence.
use super::*;
use crate::types::AgentKind;

#[test]
fn agy_family_bestof_default_racer_is_pinned_to_unspent_family() {
    let home = crate::support::test_env::TestHome::new();
    let _cache = crate::routing::quota::live::CacheDirGuard::set(home.path());
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Antigravity]);
    let _models = crate::agent::model_validation::MockServedModelsGuard::set(
        AgentKind::Antigravity,
        Some(vec![
            "gemini-4-argon-medium".into(),
            "claude-opus-5-5-medium".into(),
        ]),
    );
    crate::routing::scores::test_support::seed_live();
    let args = RunArgs {
        agent_name: "agy".into(),
        prompt: "say hi".into(),
        ..Default::default()
    };
    let store = std::sync::Arc::new(Store::open_memory().expect("store"));
    let mut candidate = super::super::super::advice_route::report(Some(&store), &args)
        .candidates
        .into_iter()
        .find(|candidate| candidate.agent == "agy")
        .expect("agy");
    candidate.model = None;
    candidate.pinned = false;
    crate::routing::rate_limit::mark_untrusted_for_model(
        &AgentKind::Antigravity,
        None,
        Some("gemini-4-argon-medium"),
        "Individual quota reached. Resets in 36h29m20s.",
    );
    let mut child = racer_args(&args, &candidate);
    let setup =
        crate::cmd::run::run_dispatch_resolve::resolve_agent_setup(&store, &mut child, None)
            .expect("checked racer");
    assert_eq!(
        setup.effective_model.as_deref(),
        Some("claude-opus-5-5-medium")
    );
}
