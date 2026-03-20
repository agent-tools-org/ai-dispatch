// Cost estimation for AI agent tasks.
// Maps model names to per-token pricing, computes task cost from token counts.
use crate::cmd::config;
use crate::types::AgentKind;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Price per 1M tokens (input, output) in USD
#[derive(Clone, Copy)]
struct ModelPricing {
    input_per_m: f64,
    output_per_m: f64,
}

static PRICING_OVERRIDES: OnceLock<HashMap<(AgentKind, String), ModelPricing>> = OnceLock::new();

/// Estimate cost in USD from total token count and model name.
/// Uses blended rate (assumes ~70% input, ~30% output) when breakdown unavailable.
pub fn estimate_cost(tokens: i64, model: Option<&str>, agent: AgentKind) -> Option<f64> {
    let pricing = resolve_pricing(model, agent)?;
    let blended_per_m = pricing.input_per_m * 0.7 + pricing.output_per_m * 0.3;
    Some(tokens as f64 * blended_per_m / 1_000_000.0)
}

/// Format cost for display: "$0.0012" or "free"
pub fn format_cost(cost_usd: Option<f64>) -> String {
    match cost_usd {
        Some(c) if c < 0.0001 => "free".to_string(),
        Some(c) if c < 0.01 => format!("${:.4}", c),
        Some(c) => format!("${:.2}", c),
        None => "—".to_string(),
    }
}

pub fn format_cost_label(cost_usd: Option<f64>, agent: AgentKind) -> String {
    match agent {
        AgentKind::Cursor => match cost_usd {
            Some(c) if c > 0.0 => format_cost(cost_usd),
            _ => "subscription".to_string(),
        },
        AgentKind::Kilo if cost_usd == Some(0.0) => "included".to_string(),
        AgentKind::Kilo => format_cost(cost_usd),
        _ => format_cost(cost_usd),
    }
}
fn resolve_pricing(model: Option<&str>, agent: AgentKind) -> Option<ModelPricing> {
    if let Some(m) = model {
        return model_pricing(m, agent);
    }
    // Default pricing by agent
    match agent {
        AgentKind::Gemini => model_pricing("gemini-2.5-flash", agent),
        AgentKind::Codex => model_pricing("gpt-4.1", agent),
        AgentKind::OpenCode => None, // Unknown without model
        AgentKind::Cursor => Some(ModelPricing {
            input_per_m: 0.0,
            output_per_m: 0.0,
        }),
        AgentKind::Kilo => Some(ModelPricing {
            input_per_m: 0.0,
            output_per_m: 0.0,
        }),
        AgentKind::Codebuff => None, // Cost tracked by codebuff SDK
        AgentKind::Droid => None,    // BYOK (API key)
        AgentKind::Oz => None,       // Reports own cost via events
        AgentKind::Custom => None,
    }
}

fn pricing_overrides() -> &'static HashMap<(AgentKind, String), ModelPricing> {
    PRICING_OVERRIDES.get_or_init(|| {
        config::load_pricing_overrides()
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
    // Normalize model name for matching
    let m = model.to_lowercase();
    let p = if m.contains("gpt-4.1") && !m.contains("mini") && !m.contains("nano") {
        ModelPricing {
            input_per_m: 2.0,
            output_per_m: 8.0,
        }
    } else if m.contains("gpt-5.4") {
        ModelPricing {
            input_per_m: 2.5,
            output_per_m: 15.0,
        }
    } else if m.contains("gpt-5") && m.contains("mini") {
        ModelPricing {
            input_per_m: 0.25,
            output_per_m: 2.0,
        }
    } else if m.contains("gpt-5") && m.contains("nano") {
        ModelPricing {
            input_per_m: 0.05,
            output_per_m: 0.40,
        }
    } else if m.contains("gpt-5") {
        ModelPricing {
            input_per_m: 1.25,
            output_per_m: 10.0,
        }
    } else if m.contains("gpt-4.1-mini") {
        ModelPricing {
            input_per_m: 0.4,
            output_per_m: 1.6,
        }
    } else if m.contains("gpt-4.1-nano") {
        ModelPricing {
            input_per_m: 0.1,
            output_per_m: 0.4,
        }
    } else if m.contains("gemini-2.5-flash") {
        ModelPricing {
            input_per_m: 0.30,
            output_per_m: 2.50,
        }
    } else if m.contains("gemini-2.5-pro") {
        ModelPricing {
            input_per_m: 1.25,
            output_per_m: 10.0,
        }
    } else if m.contains("claude-sonnet-4") || m.contains("claude-4-sonnet") {
        ModelPricing {
            input_per_m: 3.0,
            output_per_m: 15.0,
        }
    } else if m.contains("claude-opus-4") || m.contains("claude-opus-4-6") {
        ModelPricing {
            input_per_m: 15.0,
            output_per_m: 75.0,
        }
    } else if m.contains("o3-mini") || m.contains("o4-mini") {
        ModelPricing {
            input_per_m: 1.10,
            output_per_m: 4.40,
        }
    } else if m.contains("grok") {
        ModelPricing {
            input_per_m: 3.0,
            output_per_m: 15.0,
        }
    } else if m.contains("free")
        && (m.contains("mimo")
            || m.contains("nemotron")
            || m.contains("minimax")
            || m.contains("kilo"))
    {
        return Some(ModelPricing {
            input_per_m: 0.0,
            output_per_m: 0.0,
        });
    } else if m.contains("glm-4") || m.contains("glm-5") || m.contains("kimi-k2") {
        ModelPricing {
            input_per_m: 0.42,
            output_per_m: 2.10,
        }
    } else if m.contains("composer-2") {
        ModelPricing {
            input_per_m: 0.50,
            output_per_m: 2.50,
        }
    } else {
        return None;
    };
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kilo_and_free_models_zero_cost() {
        assert_eq!(
            estimate_cost(
                100_000,
                Some("opencode/mimo-v2-flash-free"),
                AgentKind::OpenCode
            ),
            Some(0.0)
        );
        assert_eq!(estimate_cost(100_000, None, AgentKind::Kilo), Some(0.0));
        assert_eq!(
            estimate_cost(100_000, Some("kilo/kilo/auto-free"), AgentKind::Kilo),
            Some(0.0)
        );
    }

    #[test]
    fn gpt41_cost_estimate() {
        let cost = estimate_cost(1_000_000, Some("gpt-4.1"), AgentKind::Codex).unwrap();
        assert!((cost - 3.8).abs() < 0.01);
    }

    #[test]
    fn composer2_cost_estimate() {
        let cost = estimate_cost(1_000_000, Some("composer-2"), AgentKind::Cursor).unwrap();
        assert!((cost - 1.10).abs() < 0.01);
    }

    #[test]
    fn unknown_model_returns_none() {
        let cost = estimate_cost(1000, Some("unknown-model"), AgentKind::OpenCode);
        assert!(cost.is_none());
    }

    #[test]
    fn format_cost_variants() {
        assert_eq!(format_cost(Some(0.0)), "free");
        assert_eq!(format_cost(Some(0.0038)), "$0.0038");
        assert_eq!(format_cost(Some(1.23)), "$1.23");
        assert_eq!(format_cost(None), "—");
    }

    #[test]
    fn format_cost_label_special_cases() {
        assert_eq!(format_cost_label(Some(1.0), AgentKind::Cursor), "$1.00");
        assert_eq!(format_cost_label(None, AgentKind::Cursor), "subscription");
        assert_eq!(format_cost_label(Some(0.0), AgentKind::Kilo), "included");
    }

    #[test]
    fn format_cost_label_codebuff() {
        assert_eq!(format_cost_label(Some(1.5), AgentKind::Codebuff), "$1.50");
    }

    #[test]
    fn new_model_pricing_entries() {
        let pricing = model_pricing("claude-sonnet-4", AgentKind::Custom).unwrap();
        assert_eq!(pricing.input_per_m, 3.0);
        assert_eq!(pricing.output_per_m, 15.0);
        let pricing = model_pricing("gpt-5", AgentKind::Codex).unwrap();
        assert_eq!(pricing.input_per_m, 1.25);
        assert_eq!(pricing.output_per_m, 10.0);
        let pricing = model_pricing("gpt-5.4", AgentKind::Codex).unwrap();
        assert_eq!(pricing.input_per_m, 2.5);
        assert_eq!(pricing.output_per_m, 15.0);
        let pricing = model_pricing("gpt-5-mini", AgentKind::Codex).unwrap();
        assert_eq!(pricing.input_per_m, 0.25);
        assert_eq!(pricing.output_per_m, 2.0);
        let pricing = model_pricing("o3-mini", AgentKind::Custom).unwrap();
        assert_eq!(pricing.input_per_m, 1.10);
        assert_eq!(pricing.output_per_m, 4.40);
    }
}
