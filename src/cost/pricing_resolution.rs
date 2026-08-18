// Resolves explicit model pricing while preserving priced, included, and unknown states.
// Exports: resolve_model_pricing().
// Deps: price feed, built-in pricing, model catalog, provider metering.

use super::{feed_index, price_feed, pricing_builtin, ModelPricing};
use crate::model_catalog::{self, AGENT_MODELS};
use crate::types::{provider_for_cli, AgentKind, MeteringShape};

pub(super) fn resolve_model_pricing(model: &str, agent: AgentKind) -> Option<ModelPricing> {
    // Subscription metering is included regardless of the catalog row.
    if matches!(provider_for_cli(agent).1, MeteringShape::Subscription) {
        return Some(ModelPricing {
            input_per_m: 0.0,
            output_per_m: 0.0,
        });
    }
    // A static catalog row's own price is authoritative: discovery, the feed,
    // and similar-model builtin rates must not bypass or outrank it.
    if let Some(pricing) = static_catalog_pricing(model, agent) {
        return Some(pricing);
    }
    if model_catalog::is_unpriced_discovered_model(agent, model) {
        return declared_free_name_pricing(model);
    }
    if let Some((feed, index)) = feed_index()
        && let Some(entry) = price_feed::feed_lookup(&feed, &index, model)
    {
        return Some(ModelPricing {
            input_per_m: entry.input_per_mtok,
            output_per_m: entry.output_per_mtok,
        });
    }
    pricing_builtin::for_model_lower(&model.to_lowercase())
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

/// Self-declared free names stay $0.00. Similar-model rates must not fill in.
fn declared_free_name_pricing(model: &str) -> Option<ModelPricing> {
    pricing_builtin::is_free_named(model).then_some(ModelPricing {
        input_per_m: 0.0,
        output_per_m: 0.0,
    })
}

#[cfg(test)]
#[path = "pricing_resolution_tests.rs"]
mod tests;
