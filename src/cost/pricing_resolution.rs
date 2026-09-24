// Resolves a model price from exact matches only: static catalog, override, or price feed.
// Exports: resolve_model_pricing(), subscription_pricing(), exact_feed_pricing().
// Deps: price feed, pricing overrides, model catalog, provider metering.

use super::{feed_index, override_pricing, price_feed, ModelPricing};
use crate::model_catalog::AGENT_MODELS;
use crate::types::{provider_for_cli, AgentKind, MeteringShape};

/// A price exists only as an exact static catalog row, an explicit pricing
/// override, or an exact price-feed id/alias. Anything else is unknown (`None`):
/// no substring, prefix, family, or free-name guess fills it in.
pub(super) fn resolve_model_pricing(model: &str, agent: AgentKind) -> Option<ModelPricing> {
    if let Some(included) = subscription_pricing(agent) {
        return Some(included);
    }
    // A static catalog row's own price is authoritative.
    static_catalog_pricing(model, agent)
        .or_else(|| override_pricing(model, agent))
        .or_else(|| exact_feed_pricing(model))
}

/// Subscription metering is included (zero marginal cost) whatever the model.
pub(super) fn subscription_pricing(agent: AgentKind) -> Option<ModelPricing> {
    matches!(provider_for_cli(agent).1, MeteringShape::Subscription).then_some(ModelPricing {
        input_per_m: 0.0,
        output_per_m: 0.0,
    })
}

pub(super) fn exact_feed_pricing(model: &str) -> Option<ModelPricing> {
    let (feed, index) = feed_index()?;
    let entry = price_feed::feed_lookup(&feed, &index, model)?;
    Some(ModelPricing {
        input_per_m: entry.input_per_mtok,
        output_per_m: entry.output_per_mtok,
    })
}

fn static_catalog_pricing(model: &str, agent: AgentKind) -> Option<ModelPricing> {
    let row = AGENT_MODELS
        .iter()
        .find(|known| known.agent == agent && known.model.eq_ignore_ascii_case(model))?;
    // tier "unknown" stores 0.0/0.0 as "no figure", not free (grok).
    if row.tier == "unknown" {
        return None;
    }
    Some(ModelPricing {
        input_per_m: row.input_per_m,
        output_per_m: row.output_per_m,
    })
}

#[cfg(test)]
#[path = "pricing_resolution_tests.rs"]
mod tests;
