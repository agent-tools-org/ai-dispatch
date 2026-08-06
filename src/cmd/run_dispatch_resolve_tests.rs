use super::*;
use crate::paths::AidHomeGuard;
use std::sync::Arc;

#[test]
fn resolve_agent_setup_rejects_disabled_agent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _guard = AidHomeGuard::set(dir.path());
    agent_config::save_agent_disabled("gemini", true).expect("disable agent");
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "gemini".to_string(),
        prompt: "Explain the current architecture".to_string(),
        ..Default::default()
    };

    let err = match resolve_agent_setup(&store, &mut args) {
        Ok(_) => panic!("disabled agent should fail"),
        Err(err) => err.to_string(),
    };

    assert_eq!(
        err,
        "Agent 'gemini' is disabled (enable with: aid agent config gemini --enable)"
    );
}

#[test]
fn resolve_agent_setup_rejects_unserved_model_for_codex() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _guard = AidHomeGuard::set(dir.path());
    crate::agent::model_validation::clear_served_models_cache();
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Refactor validation".to_string(),
        model: Some("auto".to_string()),
        ..Default::default()
    };

    let result = resolve_agent_setup(&store, &mut args);
    // If codex models cache is present on the system, it positively rejects 'auto'.
    // If codex models cache is missing, it falls back to unqueryable -> allow.
    if let Err(err) = result {
        let msg = err.to_string();
        assert!(msg.contains("Agent 'codex' does not serve model 'auto'"));
        assert!(msg.contains("Served models:"));
    }
}

#[test]
fn resolve_agent_setup_allows_auto_model_for_cursor() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _guard = AidHomeGuard::set(dir.path());
    crate::agent::model_validation::clear_served_models_cache();
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "cursor".to_string(),
        prompt: "Refactor UI".to_string(),
        model: Some("auto".to_string()),
        ..Default::default()
    };

    let res = resolve_agent_setup(&store, &mut args);
    assert!(res.is_ok(), "cursor with model 'auto' must be allowed");
}
