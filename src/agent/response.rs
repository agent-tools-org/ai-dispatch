// Agent-specific response extraction for persisted and buffered output.
// Exports: extract_response for watcher and renderer consumers.
// Deps: AgentKind and the Grok/Gemini envelope parsers.

use crate::types::AgentKind;

pub(crate) fn extract_response(kind: AgentKind, output: &str) -> Option<String> {
    match kind {
        AgentKind::Grok => super::grok::extract_response(output),
        AgentKind::Gemini | AgentKind::Antigravity => super::gemini::extract_response(output),
        _ => None,
    }
}
