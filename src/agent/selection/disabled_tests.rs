// Tests for disabled-agent filtering in auto-selection and fallback chains.
// Exports: test cases only.
// Deps: selection helpers, agent_config, Store, AidHomeGuard.

use super::{coding_fallback_for, select_agent_from};
use crate::agent::RunOpts;
use crate::agent_config;
use crate::paths::AidHomeGuard;
use crate::store::Store;
use crate::types::AgentKind;

fn opts() -> RunOpts {
    RunOpts {
        dir: None,
        output: None,
        model: None,
        result_file: None,
        budget: false,
        read_only: false,
        sandbox: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    }
}

#[test]
fn selection_skips_disabled_available_candidate() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _guard = AidHomeGuard::set(dir.path());
    agent_config::save_agent_disabled("gemini", true).expect("disable agent");
    let store = Store::open_memory().expect("store");

    let (selected, reason) = select_agent_from(
        "Explain the authentication flow and compare the docs?",
        &opts(),
        &[AgentKind::Gemini, AgentKind::Qwen],
        &store,
        None,
    );

    assert_eq!(selected, AgentKind::Qwen.as_str());
    assert!(reason.contains("qwen"));
}

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

    let result = coding_fallback_for(&AgentKind::Gemini, None, None);

    assert_eq!(result, Some(AgentKind::Codex));
}
