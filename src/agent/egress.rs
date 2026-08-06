// Resolve data-egress tier for a named agent (builtin or custom).
// Exports: resolve_agent_egress, require_local_egress.
// Deps: registry, custom config, types provider egress helpers.

use anyhow::Result;

use super::custom::CustomAgentConfig;
use super::registry;
use crate::types::{
    egress_for_base_url, egress_for_cli, AgentKind, EgressTier,
};

/// Where data goes for this agent name. Built-ins use their default provider;
/// custom agents use an established `base_url` only — hand-set `trust_tier` is
/// not evidence.
pub fn resolve_agent_egress(agent_name: &str) -> EgressTier {
    if let Some(kind) = AgentKind::parse_str(agent_name) {
        return egress_for_cli(kind);
    }
    registry::load_custom_agents()
        .get(agent_name)
        .map(custom_egress)
        .unwrap_or(EgressTier::Unknown)
}

fn custom_egress(config: &CustomAgentConfig) -> EgressTier {
    match config.base_url.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(url) => egress_for_base_url(url),
        None => EgressTier::Unknown,
    }
}

/// Fail when `--egress local` was declared and this agent does not reach a
/// loopback endpoint.
pub fn require_local_egress(agent_name: &str) -> Result<()> {
    let tier = resolve_agent_egress(agent_name);
    if tier.admits_local() {
        return Ok(());
    }
    let detail = egress_detail(agent_name);
    anyhow::bail!(
        "Agent '{agent_name}' is not eligible for --egress local: {detail} is {} \
         (only a provider whose established endpoint is localhost/127.0.0.1 qualifies; \
         every current built-in agent is third-party or unknown, and a hand-set \
         trust_tier is not evidence)",
        tier.label()
    );
}

fn egress_detail(agent_name: &str) -> String {
    if let Some(kind) = AgentKind::parse_str(agent_name) {
        return format!("provider '{}'", crate::types::provider_for_cli(kind).0.as_str());
    }
    match registry::load_custom_agents().get(agent_name).and_then(|c| c.base_url.as_deref()) {
        Some(url) => format!("endpoint '{url}'"),
        None => "provider 'unknown'".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_codex_is_third_party_not_local() {
        assert_eq!(resolve_agent_egress("codex"), EgressTier::ThirdParty);
        assert!(require_local_egress("codex").is_err());
    }

    #[test]
    fn unknown_name_is_unknown() {
        assert_eq!(resolve_agent_egress("no-such-agent-xyz"), EgressTier::Unknown);
    }
}
