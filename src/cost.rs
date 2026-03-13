// Cost estimation for AI agent tasks.
// Maps model names to per-token pricing, computes task cost from token counts.

use crate::types::AgentKind;

/// Price per 1M tokens (input, output) in USD
struct ModelPricing {
    input_per_m: f64,
    output_per_m: f64,
}

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
        None => "-".to_string(),
    }
}

fn resolve_pricing(model: Option<&str>, agent: AgentKind) -> Option<ModelPricing> {
    if let Some(m) = model {
        return model_pricing(m);
    }
    // Default pricing by agent
    match agent {
        AgentKind::Gemini => model_pricing("gemini-2.5-flash"),
        AgentKind::Codex => model_pricing("gpt-4.1"),
        AgentKind::OpenCode => None, // Unknown without model
        AgentKind::Cursor => None,
    }
}

fn model_pricing(model: &str) -> Option<ModelPricing> {
    // Normalize model name for matching
    let m = model.to_lowercase();
    let p = if m.contains("gpt-4.1") && !m.contains("mini") && !m.contains("nano") {
        ModelPricing { input_per_m: 2.0, output_per_m: 8.0 }
    } else if m.contains("gpt-4.1-mini") {
        ModelPricing { input_per_m: 0.4, output_per_m: 1.6 }
    } else if m.contains("gpt-4.1-nano") {
        ModelPricing { input_per_m: 0.1, output_per_m: 0.4 }
    } else if m.contains("gemini-2.5-flash") {
        ModelPricing { input_per_m: 0.15, output_per_m: 0.60 }
    } else if m.contains("gemini-2.5-pro") {
        ModelPricing { input_per_m: 1.25, output_per_m: 10.0 }
    } else if m.contains("free") && (m.contains("mimo") || m.contains("nemotron") || m.contains("minimax")) {
        return Some(ModelPricing { input_per_m: 0.0, output_per_m: 0.0 });
    } else if m.contains("glm-5") || m.contains("kimi-k2.5") {
        ModelPricing { input_per_m: 0.5, output_per_m: 2.0 }
    } else {
        return None;
    };
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_model_costs_zero() {
        let cost = estimate_cost(100_000, Some("opencode/mimo-v2-flash-free"), AgentKind::OpenCode);
        assert_eq!(cost, Some(0.0));
    }

    #[test]
    fn gpt41_cost_estimate() {
        let cost = estimate_cost(1_000_000, Some("gpt-4.1"), AgentKind::Codex).unwrap();
        // 0.7 * 2.0 + 0.3 * 8.0 = 1.4 + 2.4 = 3.8
        assert!((cost - 3.8).abs() < 0.01);
    }

    #[test]
    fn unknown_model_returns_none() {
        let cost = estimate_cost(1000, Some("unknown-model"), AgentKind::OpenCode);
        assert!(cost.is_none());
    }

    #[test]
    fn format_free() {
        assert_eq!(format_cost(Some(0.0)), "free");
    }

    #[test]
    fn format_small_cost() {
        assert_eq!(format_cost(Some(0.0038)), "$0.0038");
    }

    #[test]
    fn format_normal_cost() {
        assert_eq!(format_cost(Some(1.23)), "$1.23");
    }

    #[test]
    fn format_none() {
        assert_eq!(format_cost(None), "-");
    }
}
