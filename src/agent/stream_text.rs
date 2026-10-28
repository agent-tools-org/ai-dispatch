// Shared text extraction for Qwen and Antigravity JSON envelopes.
// Deps: serde_json; preserves array order and text/content/parts precedence.

use serde_json::Value;

pub(super) fn extract_text_payload(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let parts = items
                .iter()
                .filter_map(|item| extract_text_payload(Some(item)))
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.concat())
        }
        Value::Object(map) => {
            for key in ["text", "content", "parts"] {
                if let Some(text) = map
                    .get(key)
                    .and_then(|item| extract_text_payload(Some(item)))
                    && !text.is_empty()
                {
                    return Some(text);
                }
            }
            None
        }
        _ => None,
    }
}
