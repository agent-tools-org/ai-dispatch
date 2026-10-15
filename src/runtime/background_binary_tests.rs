// Tests for background agent binary preflight behavior.
// Covers missing built-in agent binaries before command construction.
// Deps: worker RunArgs and AgentKind parsing.

use super::ensure_agent_binary_available_with;
use crate::cmd::run::RunArgs;

#[test]
fn background_preflight_rejects_missing_kilo_binary() {
    let args = RunArgs { agent_name: "kilo".to_string(), ..Default::default() };

    let err = ensure_agent_binary_available_with(&args, |_| false).unwrap_err();

    assert_eq!(
        err.to_string(),
        "Agent 'kilo' not found: binary 'kilo' missing from PATH"
    );
}

#[test]
fn background_preflight_skips_containerized_runs() {
    let args = RunArgs {
        agent_name: "kilo".to_string(),
        container: Some("ubuntu:latest".to_string()),
        ..Default::default()
    };

    ensure_agent_binary_available_with(&args, |_| false).unwrap();
}
