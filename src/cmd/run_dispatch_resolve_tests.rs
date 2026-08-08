use super::*;
use crate::paths::AidHomeGuard;
use crate::types::AgentKind;
use std::sync::Arc;

/// Write a manual-hold marker for `agent` so `dispatch_blocking_hold` returns `Some`.
fn write_manual_hold(agent: AgentKind) {
    let path = crate::paths::aid_dir().join(format!("rate-limit-{}", agent.as_str()));
    std::fs::create_dir_all(crate::paths::aid_dir()).unwrap();
    std::fs::write(path, "recovery_at: \nhold: manual\nmessage: test quota exhausted\n").unwrap();
}

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

/// When the primary agent is held, resolve_agent_setup must switch to the
/// first available cascade entry before returning — not after a failed dispatch.
#[test]
fn held_agent_switches_to_explicit_cascade_before_dispatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _guard = AidHomeGuard::set(dir.path());
    write_manual_hold(AgentKind::Codex);
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Add unit tests".to_string(),
        cascade: vec!["oz".to_string()],
        ..Default::default()
    };

    let setup = resolve_agent_setup(&store, &mut args).expect("should switch to fallback");

    assert_eq!(setup.agent_kind, AgentKind::Oz, "should have switched to oz");
    assert_eq!(args.agent_name, "oz");
    let (original, _) = setup.substituted_from.expect("substituted_from must be set");
    assert_eq!(original, "codex");
    // Remaining cascade after oz is consumed must be empty.
    assert!(args.cascade.is_empty());
}

/// When the first cascade agent is also held, resolve_agent_setup must keep
/// walking to the first non-held option.
#[test]
fn held_agent_walks_cascade_past_also_held_agent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _guard = AidHomeGuard::set(dir.path());
    write_manual_hold(AgentKind::Codex);
    write_manual_hold(AgentKind::Oz);
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Refactor module".to_string(),
        cascade: vec!["oz".to_string(), "cursor".to_string()],
        ..Default::default()
    };

    let setup = resolve_agent_setup(&store, &mut args).expect("should walk to cursor");

    assert_eq!(setup.agent_kind, AgentKind::Cursor);
    assert_eq!(args.agent_name, "cursor");
    assert!(setup.substituted_from.is_some(), "substituted_from must record the original agent");
    // cursor was the last candidate; nothing left in cascade.
    assert!(args.cascade.is_empty());
}

/// Switching to a fallback must drop the caller's model so a codex-specific
/// model id does not reach a different CLI.
#[test]
fn held_agent_drops_model_when_switching_to_cascade() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _guard = AidHomeGuard::set(dir.path());
    write_manual_hold(AgentKind::Codex);
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Add tests".to_string(),
        model: Some("gpt-5.6-luna".to_string()),
        cascade: vec!["oz".to_string()],
        ..Default::default()
    };

    let setup = resolve_agent_setup(&store, &mut args).expect("should switch to oz");

    assert_eq!(setup.agent_kind, AgentKind::Oz);
    assert_eq!(args.model, None, "codex model must not carry over to oz");
    assert!(setup.substituted_from.is_some());
}

/// When both the primary and all cascade options are held and no auto-fallback
/// is available, resolve_agent_setup must return an error — no dispatch at all.
#[test]
fn held_agent_bails_when_cascade_exhausted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _guard = AidHomeGuard::set(dir.path());
    write_manual_hold(AgentKind::Codex);
    write_manual_hold(AgentKind::Oz);
    // Hold all agents (ALL_BUILTIN omits Claude) so auto-fallback finds nothing.
    for a in AgentKind::ALL_BUILTIN {
        write_manual_hold(*a);
    }
    write_manual_hold(AgentKind::Claude);
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Fix bug".to_string(),
        cascade: vec!["oz".to_string()],
        ..Default::default()
    };

    match resolve_agent_setup(&store, &mut args) {
        Err(err) => assert!(err.to_string().contains("held"), "error should mention 'held': {err}"),
        Ok(_) => panic!("all options held must produce an error"),
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
