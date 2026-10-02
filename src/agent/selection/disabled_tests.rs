// Existing disabled-agent fallback coverage; advice owns routing tests.
// Exports: test cases only.
// Deps: selection helpers, agent_config, Store, AidHomeGuard.

use crate::agent_config;
use crate::paths::AidHomeGuard;
use crate::types::AgentKind;

#[test]
fn fallback_chain_skips_disabled_agent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _guard = AidHomeGuard::set(dir.path());
    let _agents = crate::agent::DetectAgentsGuard::set(vec![
        AgentKind::Gemini,
        AgentKind::Qwen,
        AgentKind::Codex,
    ]);
    agent_config::save_agent_disabled("qwen", true).expect("disable agent");

    let result = automatic_candidate(
        None,
        &RunArgs {
            agent_name: "gemini".into(),
            prompt: "Implement a feature".into(),
            ..Default::default()
        },
    )
    .and_then(|candidate| candidate.kind());

    assert_eq!(result, Some(AgentKind::Codex));
}
