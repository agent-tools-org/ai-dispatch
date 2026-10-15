// Credit refusals from provider error fields only, never agent prose or tool arguments.
// Normalizes provider error-code separators before matching billing signatures.
use serde_json::Value;

pub(super) fn credit_refusal(value: &Value) -> bool {
    if !matches!(value["type"].as_str(), Some("error" | "assistant" | "result")) {
        return false;
    }
    let error = &value["error"];
    let mut fields = vec![
        error.as_str(),
        error["type"].as_str(),
        error["code"].as_str(),
        error["message"].as_str(),
        value["error_code"].as_str(),
    ];
    if value["type"] == "error" {
        fields.push(value["message"].as_str());
    }
    if value["type"] == "result"
        && value["is_error"] == true
        && let Some(errors) = value["errors"].as_array()
    {
        fields.extend(errors.iter().filter_map(Value::as_str).map(Some));
    }
    fields.into_iter().flatten().any(|field| {
        let field = field.to_ascii_lowercase().replace(['-', '_'], " ");
        [
            "credit balance is too low",
            "credit balance too low",
            "insufficient credit",
            "billing error",
        ]
        .iter()
        .any(|pattern| field.contains(pattern))
    })
}
