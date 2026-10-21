// Route-aware marker classification preserves provider reset evidence.
// Custom Codex providers do not inherit the default plan's fallback reset clock.
use super::*;
use crate::routing::rate_limit::signatures::{
    QuotaRecovery, match_quota_signature_for_agent, recovery_for_agent,
};

pub(super) fn write_marker(
    path: &std::path::Path,
    message: &str,
    provider: Option<&str>,
    agent: AgentKind,
    signature_agent: AgentKind,
    custom_name: Option<&str>,
) {
    write_marker_with_hold(
        path,
        message,
        provider,
        agent,
        signature_agent,
        classify_marker_hold(message, agent, custom_name, signature_agent),
    );
}

fn classify_marker_hold(
    message: &str,
    agent: AgentKind,
    custom_name: Option<&str>,
    signature_agent: AgentKind,
) -> Hold {
    if crate::routing::availability::evidence_agent(agent, custom_name) != agent
        && !crate::routing::capacity::is_capacity(message)
        && recovery_for_agent(message, signature_agent).is_none()
        && matches!(
            match_quota_signature_for_agent(message, signature_agent),
            Some(QuotaRecovery::After(_))
        )
    {
        return Hold::NeedsHuman;
    }
    classify_hold(message, signature_agent)
}

#[cfg(test)]
#[path = "hold_tests.rs"]
mod tests;
