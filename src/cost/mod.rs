// Cost estimation for AI agent tasks.
// Maps model names to per-token pricing, computes task cost from token counts.
// Deps: model_catalog, store::Store, types::AgentKind, price_feed

mod price_feed;
mod pricing_resolution;

use crate::model_catalog;
use crate::store::Store;
use crate::types::AgentKind;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// Price per 1M tokens (input, output) in USD
#[derive(Clone, Copy)]
pub(crate) struct ModelPricing {
    pub(crate) input_per_m: f64,
    pub(crate) output_per_m: f64,
}

#[cfg(not(test))]
static PRICING_OVERRIDES: OnceLock<HashMap<(AgentKind, String), ModelPricing>> = OnceLock::new();

#[cfg(test)]
thread_local! {
    static TEST_PRICING_OVERRIDES: std::cell::RefCell<Option<Arc<HashMap<(AgentKind, String), ModelPricing>>>> = const { std::cell::RefCell::new(None) };
}

/// Most recent observed Gemini model from the task DB; unset means unknown cost.
static GEMINI_DEFAULT_MODEL_CACHE: OnceLock<Option<String>> = OnceLock::new();

/// Populate [`GEMINI_DEFAULT_MODEL_CACHE`] once per process from [`Store::latest_default_model`].
pub fn warm_gemini_default_from_store(store: &Store) {
    let _ = GEMINI_DEFAULT_MODEL_CACHE
        .get_or_init(|| store.latest_default_model(AgentKind::Gemini).unwrap_or_default());
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

/// Whether cost tracking can price this route: the same resolution
/// [`estimate_cost`] uses. `false` means a task on it records an unknown cost.
pub fn has_known_price(model: Option<&str>, agent: AgentKind) -> bool {
    resolve_pricing(model, agent).is_some()
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

/// A cost total over tasks: the known sum, and how many tasks ran with an
/// unknown (NULL) cost. Unknown costs are never counted as $0.
pub fn format_cost_total(known_sum: f64, unknown_tasks: usize) -> String {
    match unknown_tasks {
        0 => format_cost(Some(known_sum)),
        n if known_sum > 0.0 => format!("{} + {n} unknown", format_cost(Some(known_sum))),
        n => format!("unknown ({n} tasks)"),
    }
}

/// Tasks that used tokens but have no known cost.
pub fn unknown_cost_tasks<'a>(tasks: impl IntoIterator<Item = &'a crate::types::Task>) -> usize {
    tasks.into_iter()
        .filter(|task| task.cost_usd.is_none() && task.tokens.is_some_and(|tokens| tokens > 0))
        .count()
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

#[cfg(not(test))]
static FEED_INDEX: OnceLock<Mutex<Option<FeedIndex>>> = OnceLock::new();

#[cfg(test)]
thread_local! {
    static TEST_FEED_INDEX: std::cell::RefCell<Option<Option<FeedIndex>>> = const { std::cell::RefCell::new(None) };
}

type FeedIndex = (Arc<price_feed::Feed>, Arc<HashMap<String, usize>>);

fn feed_index() -> Option<FeedIndex> {
    #[cfg(not(test))]
    {
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
    #[cfg(test)]
    {
        TEST_FEED_INDEX.with(|cell| {
            let mut borrowed = cell.borrow_mut();
            if let Some(cached) = borrowed.as_ref() {
                return cached.clone();
            }
            let loaded = price_feed::load_cache()
                .map(|feed| {
                    let index = feed.index();
                    (Arc::new(feed), Arc::new(index))
                });
            *borrowed = Some(loaded.clone());
            loaded
        })
    }
}

/// Test seam: force the process feed index from a constructed feed. The real
/// cache lives under the aid home, which tests redirect, so this is how feed
/// precedence gets exercised deterministically.
#[cfg(test)]
fn set_feed_for_tests(feed: price_feed::Feed) {
    let index = feed.index();
    TEST_FEED_INDEX.with(|cell| {
        *cell.borrow_mut() = Some(Some((Arc::new(feed), Arc::new(index))));
    });
}

/// Test seam: clear the process feed index so a seeded feed cannot leak into
/// other tests running in the same process. Also used by tests outside this
/// module that assert catalog-derived pricing: a developer's real feed knows
/// prices the catalog deliberately does not carry, and a test that asserts
/// "unknown" must say which of the two it is asking about.
#[cfg(test)]
pub(crate) fn clear_feed_for_tests() {
    TEST_FEED_INDEX.with(|cell| {
        *cell.borrow_mut() = None;
    });
    TEST_PRICING_OVERRIDES.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

/// Prices the reported model; with none reported, only a model aid observed
/// run (Gemini's last recorded default, Qwen's selected model). Else unknown.
fn resolve_pricing(model: Option<&str>, agent: AgentKind) -> Option<ModelPricing> {
    let known = match (model, agent) {
        (Some(model), _) => Some(model.to_string()),
        (None, AgentKind::Gemini) => GEMINI_DEFAULT_MODEL_CACHE
            .get()
            .and_then(|stored| stored.clone())
            .filter(|model| !model.is_empty()),
        (None, AgentKind::Qwen) => crate::model_catalog::get_qwen_selected_model(),
        (None, _) => None,
    };
    match known {
        Some(model) => pricing_resolution::resolve_model_pricing(&model, agent),
        None => pricing_resolution::subscription_pricing(agent),
    }
}

fn pricing_overrides() -> Arc<HashMap<(AgentKind, String), ModelPricing>> {
    #[cfg(not(test))]
    {
        let cache = PRICING_OVERRIDES.get_or_init(|| {
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
        });
        Arc::new(cache.clone())
    }
    #[cfg(test)]
    {
        TEST_PRICING_OVERRIDES.with(|cell| {
            let mut borrowed = cell.borrow_mut();
            if let Some(cached) = borrowed.as_ref() {
                return cached.clone();
            }
            let loaded = model_catalog::load_pricing_overrides()
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
                .collect::<HashMap<_, _>>();
            let arc = Arc::new(loaded);
            *borrowed = Some(arc.clone());
            arc
        })
    }
}

/// Explicit pricing override for exactly this agent and model (case-insensitive).
fn override_pricing(model: &str, agent: AgentKind) -> Option<ModelPricing> {
    pricing_overrides().get(&(agent, model.to_lowercase())).copied()
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
