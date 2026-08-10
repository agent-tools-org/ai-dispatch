// Tests Codex idle-nudge capability declarations.
// Exports: none. Deps: CodexAgent, GeminiAgent, and the Agent trait.

use super::CodexAgent;
use crate::agent::{antigravity::AntigravityAgent, gemini::GeminiAgent, grok::GrokAgent, Agent};
use crate::types::AgentKind;

#[test]
fn codex_rejects_idle_nudges() {
    assert!(!CodexAgent.accepts_idle_nudge());
}

#[test]
fn default_agent_accepts_idle_nudges() {
    assert!(GeminiAgent.accepts_idle_nudge());
}

#[test]
fn noninteractive_agents_never_accept_idle_nudges() {
    for agent in [&AntigravityAgent as &dyn Agent, &GrokAgent] {
        assert!(!agent.accepts_interactive_input());
        assert!(!agent.accepts_idle_nudge());
    }
    assert_eq!(AntigravityAgent.kind(), AgentKind::Antigravity);
    assert_eq!(GrokAgent.kind(), AgentKind::Grok);
}
