// Tests Codex idle-nudge capability declarations.
// Exports: none. Deps: CodexAgent, GeminiAgent, and the Agent trait.

use super::CodexAgent;
use crate::agent::{gemini::GeminiAgent, Agent};

#[test]
fn codex_rejects_idle_nudges() {
    assert!(!CodexAgent.accepts_idle_nudge());
}

#[test]
fn default_agent_accepts_idle_nudges() {
    assert!(GeminiAgent.accepts_idle_nudge());
}
