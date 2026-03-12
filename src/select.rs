// Agent-selection heuristics for the `aid run auto` flow.
// Exports a small prompt classifier based on cheap string rules.

use crate::types::AgentKind;

pub fn select_agent(prompt: &str, _has_worktree: bool) -> AgentKind {
    let prompt = prompt.to_lowercase();
    if prompt.contains('?')
        || prompt.contains("what")
        || prompt.contains("how")
        || prompt.contains("research")
    {
        AgentKind::Gemini
    } else if prompt.contains("frontend")
        || prompt.contains("react")
        || prompt.contains("css")
        || prompt.contains("html")
    {
        AgentKind::Cursor
    } else if prompt.contains("rename")
        || prompt.contains("replace")
        || prompt.contains("change type")
    {
        AgentKind::OpenCode
    } else {
        AgentKind::Codex
    }
}
