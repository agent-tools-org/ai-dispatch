// Resolves explicit model pricing while preserving priced, included, and unknown states.
// Exports: resolve_model_pricing().
// Deps: price feed, built-in pricing, model catalog, provider metering.

use super::{feed_index, price_feed, pricing_builtin, ModelPricing};
use crate::model_catalog;
use crate::types::{provider_for_cli, AgentKind, MeteringShape};

pub(super) fn resolve_model_pricing(model: &str, agent: AgentKind) -> Option<ModelPricing> {
    if model_catalog::is_unpriced_discovered_model(agent, model) {
        return None;
    }
    if let Some((feed, index)) = feed_index()
        && let Some(entry) = price_feed::feed_lookup(&feed, &index, model)
    {
        return Some(ModelPricing {
            input_per_m: entry.input_per_mtok,
            output_per_m: entry.output_per_mtok,
        });
    }
    if matches!(provider_for_cli(agent).1, MeteringShape::Subscription) {
        return Some(ModelPricing {
            input_per_m: 0.0,
            output_per_m: 0.0,
        });
    }
    pricing_builtin::for_model_lower(&model.to_lowercase())
}
