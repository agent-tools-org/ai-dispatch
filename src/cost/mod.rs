// Cost estimation for AI agent tasks.
// Maps model names to per-token pricing, computes task cost from token counts.
// Deps: model_catalog, store::Store, types::AgentKind

mod pricing_builtin;

use crate::model_catalog;
use crate::store::Store;
use crate::types::AgentKind;
use std::collections::HashMap;
use std::sync::OnceLock;

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

fn resolve_pricing(model: Option<&str>, agent: AgentKind) -> Option<ModelPricing> {
    if let Some(m) = model {
        return model_pricing(m, agent);
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
        AgentKind::Copilot => Some(ModelPricing {
            input_per_m: 0.0,
            output_per_m: 0.0,
        }),
        AgentKind::OpenCode => None,
        AgentKind::Cursor => Some(ModelPricing {
            input_per_m: 0.0,
            output_per_m: 0.0,
        }),
        AgentKind::Kilo | AgentKind::MiMoCode => Some(ModelPricing {
            input_per_m: 0.0,
            output_per_m: 0.0,
        }),
        AgentKind::Claude => None,
        AgentKind::Grok => None,
        AgentKind::Codebuff => None,
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
    pricing_builtin::for_model_lower(&model.to_lowercase())
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
