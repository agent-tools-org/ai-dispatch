// Exact-route resolution ignores held routes and model-group substitution.
// Uses model-validation mocks and isolated aid configuration.
use super::*;
use crate::support::paths::AidHomeGuard;
use crate::agent::model_validation::MockServedModelsGuard;

#[test]
fn caller_owned_keeps_explicit_model_through_holds_and_budget_defaults() {
    let dir = tempfile::tempdir().expect("dir");
    let _guard = AidHomeGuard::set(dir.path());
    let _models = MockServedModelsGuard::set(AgentKind::Grok, Some(vec!["exact-model".into()]));
    std::fs::write(
        dir.path().join("rate-limit-grok"),
        "recovery_at: \nhold: manual\nmessage: test exhausted\n",
    )
    .expect("hold");
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        policy: Some("caller-owned".into()),
        agent_name: "grok".into(),
        model: Some("exact-model".into()),
        budget: true,
        cascade: vec!["agy".into()],
        ..Default::default()
    };
    let resolved = resolve_agent_setup(&store, &mut args, None).expect("exact route");
    assert_eq!(resolved.agent_kind, AgentKind::Grok);
    assert_eq!(resolved.effective_model.as_deref(), Some("exact-model"));
    assert!(resolved.substituted_from.is_none());
    assert!(!resolved.budget_active);
}

#[test]
fn caller_owned_model_not_served_is_structured_and_creates_no_row() {
    let dir = tempfile::tempdir().expect("dir");
    let _guard = AidHomeGuard::set(dir.path());
    let _models = MockServedModelsGuard::set(AgentKind::Grok, Some(vec!["served".into()]));
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        policy: Some("caller-owned".into()),
        agent_name: "grok".into(),
        model: Some("absent".into()),
        ..Default::default()
    };
    let error = match resolve_agent_setup(&store, &mut args, None) {
        Ok(_) => panic!("model should be refused"),
        Err(error) => error,
    };
    assert_eq!(
        error
            .downcast_ref::<crate::execution::Refusal>()
            .expect("refusal")
            .code,
        "model_not_served"
    );
    assert!(
        store
            .list_tasks(crate::types::TaskFilter::All)
            .expect("tasks")
            .is_empty()
    );
}

#[test]
fn caller_owned_unknown_served_models_keep_the_explicit_model() {
    let dir = tempfile::tempdir().expect("dir");
    let _guard = AidHomeGuard::set(dir.path());
    let _models = MockServedModelsGuard::set(AgentKind::Grok, None);
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        policy: Some("caller-owned".into()), agent_name: "grok".into(),
        model: Some("caller-selected-unknown".into()), ..Default::default()
    };
    let resolved = resolve_agent_setup(&store, &mut args, None).expect("unknown list is not refusal");
    assert_eq!(resolved.effective_model.as_deref(), Some("caller-selected-unknown"));
    assert_eq!(args.model, resolved.effective_model);
}
