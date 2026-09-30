// Existing fallback and cost-efficiency regression coverage.
// Routing expectations live with advise tests; fallback behavior is unchanged.
// Deps: fallback helpers, scoring, isolated AID_HOME, and pinned installed agents.

use crate::paths::{self, AidHomeGuard};
use crate::types::AgentKind;
use tempfile::TempDir;

fn isolated() -> (TempDir, AidHomeGuard) {
    let temp = TempDir::new().unwrap();
    let guard = AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(paths::aid_dir()).ok();
    (temp, guard)
}
#[test]
fn cost_efficiency_calculates_ratio() {
    let value = super::selection_scoring::cost_efficiency(9.0, 1.5);
    assert!((value - 3.6).abs() < 1e-6);
}

#[test]
fn gemini_in_fallback_chain() {
    let (_temp, _guard) = isolated();
    // Pin the detected agent set so the test is deterministic on CI hosts
    // where no agent binaries are installed on PATH.
    let _agents = crate::agent::DetectAgentsGuard::set(vec![
        AgentKind::Gemini,
        AgentKind::Qwen,
        AgentKind::Codex,
    ]);
    let result = super::coding_fallback_for(&AgentKind::Gemini, None, None);
    assert!(result.is_some(), "Gemini should have a fallback agent");
    assert_ne!(result, Some(AgentKind::Gemini));
}

#[test]
fn fallback_chain_skips_rate_limited() {
    let (_temp, _guard) = isolated();
    let _agents = crate::agent::DetectAgentsGuard::set(vec![
        AgentKind::Gemini,
        AgentKind::Qwen,
        AgentKind::Codex,
        AgentKind::Cursor,
    ]);
    crate::rate_limit::mark_rate_limited(&AgentKind::Codex, None, "quota exhausted");
    let result = super::coding_fallback_for(&AgentKind::Gemini, None, None);
    // Should skip Codex (rate-limited) and pick the next available.
    assert!(result.is_some());
    assert_ne!(result.unwrap(), AgentKind::Codex);
}
