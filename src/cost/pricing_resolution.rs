// Resolves a model price from exact matches only: override, subscription, static catalog, feed.
// Exports: resolve_model_pricing(), subscription_pricing().
// Deps: price feed, pricing overrides, model catalog, provider metering.

use super::{feed_index, override_pricing, price_feed, ModelPricing};
use crate::model_catalog::AGENT_MODELS;
use crate::types::{provider_for_cli, AgentKind, MeteringShape};

/// Precedence: an explicit user override, then subscription inclusion, then a
/// static catalog row with a real figure, then an exact price-feed id/alias on
/// a vendor's own CLI. Anything else is unknown (`None`): no substring, prefix, family, or
/// free-name guess fills it in.
pub(super) fn resolve_model_pricing(model: &str, agent: AgentKind) -> Option<ModelPricing> {
    override_pricing(model, agent)
        .or_else(|| subscription_pricing(agent))
        .or_else(|| static_catalog_pricing(model, agent))
        .or_else(|| exact_feed_pricing(model, agent))
}

/// Subscription metering is included (zero marginal cost) whatever the model.
pub(super) fn subscription_pricing(agent: AgentKind) -> Option<ModelPricing> {
    matches!(provider_for_cli(agent).1, MeteringShape::Subscription).then_some(ModelPricing {
        input_per_m: 0.0,
        output_per_m: 0.0,
        cached_input_per_m: Some(0.0),
    })
}

/// The feed carries each vendor's own per-token API rate, so it prices only
/// the vendor's own CLI. A reseller (droid, oz, opencode, ...) bills its own
/// rate; the vendor's figure there would be a guess.
fn exact_feed_pricing(model: &str, agent: AgentKind) -> Option<ModelPricing> {
    let vendor_cli = matches!(
        agent,
        AgentKind::Codex | AgentKind::Claude | AgentKind::Gemini | AgentKind::Grok
    );
    if !vendor_cli {
        return None;
    }
    let (feed, index) = feed_index()?;
    let entry = price_feed::feed_lookup(&feed, &index, model)?;
    Some(ModelPricing {
        input_per_m: entry.input_per_mtok,
        output_per_m: entry.output_per_mtok,
        cached_input_per_m: entry.cached_input_per_mtok,
    })
}

fn static_catalog_pricing(model: &str, agent: AgentKind) -> Option<ModelPricing> {
    let row = AGENT_MODELS
        .iter()
        .find(|known| known.agent == agent && known.model.eq_ignore_ascii_case(model))?;
    // 0.0/0.0 is a price only on a "free" row; elsewhere it means "no figure"
    // (droid, oz, grok). Subscription agents are priced before this lookup.
    if row.input_per_m == 0.0 && row.output_per_m == 0.0 && row.tier != "free" {
        return None;
    }
    Some(ModelPricing {
        input_per_m: row.input_per_m,
        output_per_m: row.output_per_m,
        cached_input_per_m: (row.tier == "free").then_some(0.0),
    })
}

#[cfg(test)]
#[path = "pricing_resolution_tests.rs"]
mod tests;
