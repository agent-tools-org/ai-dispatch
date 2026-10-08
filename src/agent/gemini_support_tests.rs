// Large model usage counts must not overflow while combining Gemini stats.

use super::extract_tokens;
use serde_json::json;

#[test]
fn regression_gemini_array_model_tokens_saturate() {
    let stats = json!({"stats": {"models": [
        {"tokens": {"total": i64::MAX}},
        {"tokens": {"total": 1}}
    ]}});
    assert_eq!(extract_tokens(&stats), Some(i64::MAX));
}

#[test]
fn regression_gemini_object_model_tokens_saturate() {
    let stats = json!({"stats": {"models": {
        "a": {"total_tokens": i64::MAX},
        "b": {"total_tokens": 1}
    }}});
    assert_eq!(extract_tokens(&stats), Some(i64::MAX));
}
