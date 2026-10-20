// Separate marker ownership from the CLI signature identity of custom overlays.
// Persist the signature identity so readers need not rediscover a changed configuration.

use super::marker_field;
use crate::types::AgentKind;

pub(super) fn signature_agent(owner: AgentKind, custom_name: Option<&str>) -> AgentKind {
    if owner != AgentKind::Custom {
        return owner;
    }
    crate::agent::registry::list_custom_agents()
        .into_iter()
        .find(|config| Some(config.id.as_str()) == custom_name)
        .and_then(|config| config.rate_limit_kind)
        .and_then(|kind| AgentKind::parse_str(&kind))
        .unwrap_or(AgentKind::OpenCode)
}

pub(crate) fn stored_signature_agent(content: &str, owner: AgentKind) -> AgentKind {
    if owner != AgentKind::Custom {
        return owner;
    }
    marker_field(content, "signature_agent: ")
        .as_deref()
        .and_then(AgentKind::parse_str)
        .unwrap_or(AgentKind::OpenCode)
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;
