// Dispatch model-source regression tests.
// Exports: resolver provenance coverage.
// Deps: run resolver, model validation, RunArgs, Store.

use super::super::resolve_agent_setup;
use crate::agent::model_validation::{ModelSource, MockServedModelsGuard};
use crate::cmd::run::RunArgs;
use crate::paths::AidHomeGuard;
use crate::store::Store;
use crate::types::AgentKind;
use std::sync::Arc;

#[test]
fn aid_selected_default_is_persisted_as_aid_resolved() {
    let home = tempfile::tempdir().expect("temporary aid home");
    let _home_guard = AidHomeGuard::set(home.path());
    crate::agent_config::save_agent_default_model("grok", Some("stale-aid-model"))
        .expect("save agent default");
    let _served = MockServedModelsGuard::set(
        AgentKind::Grok,
        Some(vec!["grok-4.7".to_string()]),
    );
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "grok".to_string(),
        prompt: "say hi".to_string(),
        ..Default::default()
    };

    let setup = resolve_agent_setup(&store, &mut args).expect("aid-selected model may degrade");

    assert_eq!(setup.effective_model, None);
    assert_eq!(args.model_source, ModelSource::AidResolved);
    let restored = RunArgs::from_dispatch_args_json(&args.dispatch_args_json().expect("serialize"))
        .expect("restore dispatch args");
    assert_eq!(restored.model_source, ModelSource::AidResolved);
}

#[test]
fn explicit_model_remains_user_supplied() {
    let home = tempfile::tempdir().expect("temporary aid home");
    let _home_guard = AidHomeGuard::set(home.path());
    let _served = MockServedModelsGuard::set(
        AgentKind::Grok,
        Some(vec!["explicit-model".to_string()]),
    );
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "grok".to_string(),
        prompt: "say hi".to_string(),
        model: Some("explicit-model".to_string()),
        model_source: ModelSource::UserSupplied,
        ..Default::default()
    };

    resolve_agent_setup(&store, &mut args).expect("served explicit model");

    assert_eq!(args.model_source, ModelSource::UserSupplied);
}

#[test]
fn substituted_model_is_persisted_as_aid_resolved() {
    let home = tempfile::tempdir().expect("temporary aid home");
    let _home_guard = AidHomeGuard::set(home.path());
    crate::rate_limit::mark_group_rate_limited(
        &AgentKind::Cursor,
        None,
        "premium",
        "premium quota exhausted",
    );
    let _served = MockServedModelsGuard::set(AgentKind::Cursor, Some(vec!["auto".to_string()]));
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "cursor".to_string(),
        prompt: "say hi".to_string(),
        model: Some("composer-2.5".to_string()),
        model_source: ModelSource::UserSupplied,
        ..Default::default()
    };

    let setup = resolve_agent_setup(&store, &mut args).expect("healthy replacement is usable");

    assert_eq!(setup.effective_model.as_deref(), Some("auto"));
    assert_eq!(args.model_source, ModelSource::AidResolved);
    let restored = RunArgs::from_dispatch_args_json(&args.dispatch_args_json().expect("serialize"))
        .expect("restore dispatch args");
    assert_eq!(restored.model_source, ModelSource::AidResolved);
}
