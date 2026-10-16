// Claude model attribution from result usage and versioned model names.
// Shared by stream event metadata normalization.
use serde_json::Value;

pub(super) fn extract_result_model(payload: &Value) -> Option<String> {
    payload
        .pointer("/modelUsage")
        .and_then(Value::as_object)
        .and_then(|models| {
            let use_cost = models
                .values()
                .all(|v| v.get("costUSD").and_then(Value::as_f64).is_some());
            let tokens = |v: &Value| {
                [
                    "inputTokens",
                    "outputTokens",
                    "cacheReadInputTokens",
                    "cacheCreationInputTokens",
                ]
                .iter()
                .filter_map(|key| v.get(*key).and_then(Value::as_i64))
                .map(i128::from)
                .sum::<i128>()
            };
            models
                .iter()
                .rev()
                .max_by(|(_, a), (_, b)| {
                    let cost = || {
                        a["costUSD"]
                            .as_f64()
                            .zip(b["costUSD"].as_f64())
                            .and_then(|(a, b)| a.partial_cmp(&b))
                    };
                    let cost_order = if use_cost { cost() } else { None };
                    cost_order
                        .filter(|order| !order.is_eq())
                        .unwrap_or_else(|| tokens(a).cmp(&tokens(b)))
                })
                .map(|(name, _)| name)
        })
        .map(|name| normalize_model(name))
        .or_else(|| payload.get("model").and_then(Value::as_str).map(normalize_model))
}

pub(super) fn normalize_model(model: &str) -> String {
    model.split('[').next().unwrap_or(model).to_string()
}
