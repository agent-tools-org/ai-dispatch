// Cost estimation for AI agent tasks.
// Maps model names to per-token pricing, computes task cost from token counts.
// Deps: model_catalog, store::Store, types::AgentKind, price_feed

mod price_feed;
mod pricing_builtin;

use crate::model_catalog;
use crate::store::Store;
use crate::types::{provider_for_cli, AgentKind, MeteringShape};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// Price per 1M tokens (input, output) in USD
#[derive(Clone, Copy)]
pub(crate) struct ModelPricing {
    pub(crate) input_per_m: f64,
    pub(crate) output_per_m: f64,
}

static PRICING_OVERRIDES: OnceLock<HashMap<(AgentKind, String), ModelPricing>> = OnceLock::new();

/// Most recent completed model name for Gemini from the task DB (`None` = checked, no hits).
/// Unset means warm has not run yet (`gemini_fallback_pricing` uses static fallback pricing).
static GEMINI_DEFAULT_MODEL_CACHE: OnceLock<Option<String>> = OnceLock::new();

/// Populate [`GEMINI_DEFAULT_MODEL_CACHE`] once per process from [`Store::latest_default_model`].
pub fn warm_gemini_default_from_store(store: &Store) {
    let _ = GEMINI_DEFAULT_MODEL_CACHE.get_or_init(|| match store.latest_default_model(AgentKind::Gemini) {
        Ok(m) => m,
        Err(_) => None,
    });
}

/// Refresh the price-feed cache out of band. Never blocks or fails a run; a
/// network failure keeps the old cache.
pub fn maybe_refresh_prices() {
    price_feed::maybe_refresh();
}

/// Estimate cost in USD from total token count and model name.
/// Uses blended rate (assumes ~70% input, ~30% output) when breakdown unavailable.
pub fn estimate_cost(tokens: i64, model: Option<&str>, agent: AgentKind) -> Option<f64> {
    let pricing = resolve_pricing(model, agent)?;
    let blended_per_m = pricing.input_per_m * 0.7 + pricing.output_per_m * 0.3;
    Some(tokens as f64 * blended_per_m / 1_000_000.0)
}

/// Format cost for display: "$0.0012", "free", or "unknown".
pub fn format_cost(cost_usd: Option<f64>) -> String {
    match cost_usd {
        Some(c) if c < 0.0001 => "free".to_string(),
        Some(c) if c < 0.01 => format!("${:.4}", c),
        Some(c) => format!("${:.2}", c),
        None => "unknown".to_string(),
    }
}

pub fn format_cost_label(cost_usd: Option<f64>, agent: AgentKind) -> String {
    match agent {
        AgentKind::Cursor => match cost_usd {
            Some(c) if c > 0.0 => format_cost(cost_usd),
            _ => "subscription".to_string(),
        },
        AgentKind::Copilot => match cost_usd {
            Some(c) if c > 0.0 => format_cost(cost_usd),
            _ => "subscription".to_string(),
        },
        AgentKind::Kilo | AgentKind::MiMoCode if cost_usd == Some(0.0) => "included".to_string(),
        AgentKind::Kilo | AgentKind::MiMoCode => format_cost(cost_usd),
        _ => format_cost(cost_usd),
    }
}

/// How a model's cost is established. The three states must stay distinct:
/// collapsing "unknown" into "included" is the $0.00 bug this replaces.
enum PricingSource {
    /// The feed knows this model and carries a real price.
    Priced,
    /// A flat-rate subscription: marginal cost is genuinely ~0
    /// (Cursor, Copilot — `MeteringShape::Subscription`).
    Included,
    /// The built-in offline matcher priced it.
    Builtin,
    /// Nobody knows. The caller must see unknown, never $0.00 and never "free".
    Unknown,
}

/// Cached feed lookup index, populated once per process from the local cache.
/// Refresh happens out of band (never on the dispatch path); a cold or stale
/// cache degrades to the built-in matcher instead of blocking a run.
static FEED_INDEX: OnceLock<Mutex<Option<FeedIndex>>> = OnceLock::new();

type FeedIndex = (Arc<price_feed::Feed>, Arc<HashMap<String, usize>>);

fn feed_index() -> Option<FeedIndex> {
    let cache = FEED_INDEX.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().ok()?;
    if let Some(pair) = guard.as_ref() {
        return Some(pair.clone());
    }
    // First load from the local cache; store the index so the dispatch path is
    // a lock + Arc clone, never a file read.
    let loaded = price_feed::load_cache()
        .map(|feed| {
            let index = feed.index();
            (Arc::new(feed), Arc::new(index))
        });
    if let Some(pair) = loaded {
        *guard = Some(pair.clone());
        return Some(pair);
    }
    None
}

/// Test seam: force the process feed index from a constructed feed. The real
/// cache lives under the aid home, which tests redirect, so this is how feed
/// precedence gets exercised deterministically.
#[cfg(test)]
fn set_feed_for_tests(feed: price_feed::Feed) {
    let index = feed.index();
    let cache = FEED_INDEX.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = cache.lock() {
        *guard = Some((Arc::new(feed), Arc::new(index)));
    }
}

/// Test seam: clear the process feed index so a seeded feed cannot leak into
/// other tests running in the same process. Also used by tests outside this
/// module that assert catalog-derived pricing: a developer's real feed knows
/// prices the catalog deliberately does not carry, and a test that asserts
/// "unknown" must say which of the two it is asking about.
#[cfg(test)]
pub(crate) fn clear_feed_for_tests() {
    let cache = FEED_INDEX.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = cache.lock() {
        *guard = None;
    }
}

/// The resolution outcome: priced feed first, then builtin, else unknown.
fn classify_pricing(model: &str, agent: AgentKind) -> (PricingSource, Option<ModelPricing>) {
    if let Some((feed, index)) = feed_index()
        && let Some(entry) = price_feed::feed_lookup(&feed, &index, model)
    {
        return (
            PricingSource::Priced,
            Some(ModelPricing {
                input_per_m: entry.input_per_mtok,
                output_per_m: entry.output_per_mtok,
            }),
        );
    }
    if matches!(
        provider_for_cli(agent).1,
        MeteringShape::Subscription
    ) {
        return (PricingSource::Included, Some(ModelPricing {
            input_per_m: 0.0,
            output_per_m: 0.0,
        }));
    }
    if let Some(pricing) = pricing_builtin::for_model_lower(&model.to_lowercase()) {
        return (PricingSource::Builtin, Some(pricing));
    }
    (PricingSource::Unknown, None)
}

fn resolve_pricing(model: Option<&str>, agent: AgentKind) -> Option<ModelPricing> {
    if let Some(m) = model {
        let (source, pricing) = classify_pricing(m, agent);
        return match source {
            // Priced by the feed or the built-in matcher, or a subscription
            // where marginal cost is really ~0. All three are known.
            PricingSource::Priced | PricingSource::Included | PricingSource::Builtin => pricing,
            // Unknown stays unknown — never synthesize $0.00.
            PricingSource::Unknown => None,
        };
    }
    match agent {
        AgentKind::Gemini => gemini_fallback_pricing(agent),
        AgentKind::Antigravity => None,
        AgentKind::Qwen => {
            let m = crate::model_catalog::get_qwen_selected_model()
                .unwrap_or_else(|| "coder-model".to_string());
            model_pricing(&m, agent)
        }
        AgentKind::Codex => codex_fallback_pricing(agent),
        AgentKind::CommandCode => None,
        AgentKind::Copilot | AgentKind::Cursor | AgentKind::Kilo | AgentKind::MiMoCode => {
            Some(ModelPricing {
                input_per_m: 0.0,
                output_per_m: 0.0,
            })
        }
        AgentKind::OpenCode => None,
        AgentKind::Claude => None,
        AgentKind::Grok => None,
        AgentKind::Droid => None,
        AgentKind::Oz => None,
        AgentKind::Custom => None,
    }
}

fn gemini_fallback_pricing(agent: AgentKind) -> Option<ModelPricing> {
    let model = GEMINI_DEFAULT_MODEL_CACHE
        .get()
        .and_then(|stored| stored.as_deref())
        .filter(|m| !m.is_empty());
    if let Some(m) = model {
        return model_pricing(m, agent);
    }
    model_pricing("gemini-3-flash-preview", agent)
}

/// Codex fallback pricing derived from the merged model catalog (AGENT_MODELS).
/// Picks the "standard" tier model; falls back to the first available.
fn codex_fallback_pricing(agent: AgentKind) -> Option<ModelPricing> {
    let models = model_catalog::models_for_agent(&agent);
    let model = models.iter().find(|m| m.tier == "standard").or_else(|| models.first())?;
    Some(ModelPricing {
        input_per_m: model.input_per_m,
        output_per_m: model.output_per_m,
    })
}

fn pricing_overrides() -> &'static HashMap<(AgentKind, String), ModelPricing> {
    PRICING_OVERRIDES.get_or_init(|| {
        model_catalog::load_pricing_overrides()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|model| {
                let agent = AgentKind::parse_str(&model.agent)?;
                Some((
                    (agent, model.model.to_lowercase()),
                    ModelPricing {
                        input_per_m: model.input_per_m,
                        output_per_m: model.output_per_m,
                    },
                ))
            })
            .collect()
    })
}

fn override_pricing(model: &str, agent: AgentKind) -> Option<ModelPricing> {
    let candidates = [
        model.to_lowercase(),
        model.rsplit('/').next().unwrap_or(model).to_lowercase(),
    ];
    for candidate in candidates {
        if let Some(pricing) = pricing_overrides().get(&(agent, candidate)) {
            return Some(*pricing);
        }
    }
    None
}

fn model_pricing(model: &str, agent: AgentKind) -> Option<ModelPricing> {
    if let Some(pricing) = override_pricing(model, agent) {
        return Some(pricing);
    }
    // The feed takes precedence over the built-in matcher when present.
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
