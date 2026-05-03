// Built-in substring model pricing lookup (USD per million tokens).
// Exports: for_model_lower
// Deps: super::ModelPricing

use super::ModelPricing;

/// Match `lower` (`model.to_lowercase()`) against known tiers.
pub(super) fn for_model_lower(m: &str) -> Option<ModelPricing> {
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
    } else if m.contains("gemini-3") && m.contains("flash") && m.contains("lite") {
        ModelPricing {
            input_per_m: 0.10,
            output_per_m: 0.40,
        }
    } else if m.contains("gemini-3") && m.contains("flash") {
        ModelPricing {
            input_per_m: 0.30,
            output_per_m: 2.50,
        }
    } else if m.contains("gemini-3") && m.contains("-pro") {
        ModelPricing {
            input_per_m: 1.25,
            output_per_m: 10.0,
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
    } else if m.contains("coder-model") {
        ModelPricing {
            input_per_m: 0.0,
            output_per_m: 0.0,
        }
    } else if m.contains("qwen") {
        ModelPricing {
            input_per_m: 0.30,
            output_per_m: 2.50,
        }
    } else if m == "haiku" || m.contains("claude-haiku") {
        ModelPricing {
            input_per_m: 0.8,
            output_per_m: 4.0,
        }
    } else if m == "sonnet" || m.contains("claude-sonnet-4") || m.contains("claude-4-sonnet") {
        ModelPricing {
            input_per_m: 3.0,
            output_per_m: 15.0,
        }
    } else if m == "opus" || m.contains("claude-opus-4") || m.contains("claude-opus-4-6") {
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
