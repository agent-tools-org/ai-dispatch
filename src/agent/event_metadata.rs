// Shared model and session metadata for agent stream envelopes.
// Preserves absent metadata and caller-specific model normalization.
// Deps: serde_json.

use serde_json::{Value, json};

pub(super) fn model_session(model: Option<&str>, session_id: Option<&str>) -> Option<Value> {
    if model.is_none() && session_id.is_none() {
        return None;
    }
    let mut metadata = json!({});
    if let Some(model) = model {
        metadata["model"] = json!(model);
    }
    if let Some(session_id) = session_id {
        metadata["agent_session_id"] = json!(session_id);
    }
    Some(metadata)
}
