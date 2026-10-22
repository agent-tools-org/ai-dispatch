// Execution route identity and metering regression tests.
// Covers provider pools and named API account isolation.
use super::*;

#[test]
fn a_bare_agent_name_resolves_to_a_triple() {
    let route = Route::for_cli(AgentKind::Codex);
    assert_eq!(route.cli, AgentKind::Codex);
    assert_eq!(route.provider.as_str(), "openai-chatgpt-plan");
    assert_eq!(route.id(), "codex/openai-chatgpt-plan/-");
}

#[test]
fn an_unpinned_model_reads_as_unpinned_not_as_a_default() {
    assert!(Route::for_cli(AgentKind::Codex).model.is_none());
    let pinned = Route::for_cli(AgentKind::Codex).with_model(Some("gpt-6-sol"));
    assert_eq!(pinned.id(), "codex/openai-chatgpt-plan/gpt-6-sol");
}
