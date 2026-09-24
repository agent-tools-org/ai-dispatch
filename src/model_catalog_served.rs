// Served-but-uncatalogued models: rows for every agent with a served-model probe.
// Exports: SERVED_PROBE_AGENTS, served_only_models, unrated_served_newer_than.
// Deps: served-model disk cache, static catalog rows; never invents ratings or prices.

use super::model_catalog_resolved::{ModelOrigin, ResolvedAgentModel};
use super::static_models_for_agent;
use crate::types::AgentKind;

/// Agents whose adapter overrides `Agent::served_models`. Every other builtin
/// uses the trait default (`Ok(None)`), which a test pins.
pub(crate) const SERVED_PROBE_AGENTS: &[AgentKind] = &[
    AgentKind::Codex,
    AgentKind::Grok,
    AgentKind::Cursor,
    AgentKind::Qwen,
    AgentKind::Antigravity,
    AgentKind::OpenCode,
];

/// Models the agent's CLI last reported serving that have no catalog row.
/// Read from the served-model disk cache only: an absent or expired cache is
/// "unknown" and yields no rows, never a probe or a guess.
pub(crate) fn served_only_models(agent: AgentKind) -> Vec<ResolvedAgentModel> {
    if !SERVED_PROBE_AGENTS.contains(&agent) {
        return Vec::new();
    }
    let known = static_models_for_agent(&agent);
    let description = format!(
        "Served by {}; unrated (capability and pricing unknown)",
        agent.as_str()
    );
    crate::agent::model_validation::load_from_disk_cache(agent)
        .unwrap_or_default()
        .into_iter()
        .filter(|name| !known.iter().any(|row| row.model.eq_ignore_ascii_case(name)))
        .map(|model| ResolvedAgentModel {
            agent,
            model,
            input_per_m: None,
            output_per_m: None,
            tier: "unknown".to_string(),
            description: description.clone(),
            capability: None,
            origin: ModelOrigin::Served,
        })
        .collect()
}

/// Unrated served models of the same family as `model` with a higher version.
pub(crate) fn unrated_served_newer_than(agent: AgentKind, model: &str) -> Vec<String> {
    let Some((family, version)) = family_version(model) else {
        return Vec::new();
    };
    served_only_models(agent)
        .into_iter()
        .filter(|row| {
            family_version(&row.model)
                .is_some_and(|(f, v)| f.eq_ignore_ascii_case(family) && v > version)
        })
        .map(|row| row.model)
        .collect()
}

/// Splits `gpt-5.6-sol` into family `gpt` and version `[5, 6]`. The family is
/// the text before the first digit (provider prefixes like `opencode/` kept);
/// names without a version number have no family and are never compared.
pub(crate) fn family_version(model: &str) -> Option<(&str, Vec<u32>)> {
    let start = model.find(|c: char| c.is_ascii_digit())?;
    let family = model[..start].trim_end_matches(['-', '_', '.']);
    if family.is_empty() {
        return None;
    }
    let digits = &model[start..];
    let end = digits
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(digits.len());
    let version = digits[..end]
        .split('.')
        .filter(|part| !part.is_empty())
        .map(str::parse)
        .collect::<Result<Vec<u32>, _>>()
        .ok()?;
    Some((family, version))
}

#[cfg(test)]
#[path = "model_catalog_served_tests.rs"]
mod tests;
