// Claude result usage normalization and completion events.
// Uses cumulative model counters when complete, with top-level usage as fallback.
use super::{capped_detail_with, extract_result_model};
use crate::types::*;
use serde_json::{Value, json};

const TOKEN_FIELDS: &[&str] = &[
    "tokens",
    "input_tokens",
    "uncached_input_tokens",
    "output_tokens",
    "prompt_tokens",
    "cache_creation_input_tokens",
    "cache_read_input_tokens",
];

fn has_usage(usage: &Value) -> bool {
    TOKEN_FIELDS
        .iter()
        .chain(["tokens_in", "tokens_out"].iter())
        .any(|key| usage.get(*key).and_then(Value::as_i64).is_some())
}

fn cumulative_usage(payload: &Value) -> Option<Value> {
    let models = payload.get("modelUsage")?.as_object()?;
    if models.is_empty() {
        return None;
    }
    let mut totals = [0i64; 4];
    for model in models.values() {
        let counts = [
            model.get("inputTokens")?.as_i64()?,
            model.get("outputTokens")?.as_i64()?,
            model
                .get("cacheCreationInputTokens")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            model
                .get("cacheReadInputTokens")
                .and_then(Value::as_i64)
                .unwrap_or(0),
        ];
        for (total, count) in totals.iter_mut().zip(counts) {
            *total = total.checked_add(count)?;
        }
    }
    Some(json!({
        "input_tokens": totals[0], "output_tokens": totals[1],
        "cache_creation_input_tokens": totals[2], "cache_read_input_tokens": totals[3],
    }))
}

pub(super) fn parse_result_event(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<chrono::Local>,
) -> Option<TaskEvent> {
    let payload = v
        .get("result")
        .filter(|value| value.is_object())
        .unwrap_or(v);
    let cumulative = cumulative_usage(payload);
    let usage = cumulative
        .as_ref()
        .or_else(|| payload.get("usage"))
        .unwrap_or(payload);
    let input_tokens = usage_token_count(usage, "input_tokens", "tokens_in");
    let output_tokens = usage_token_count(usage, "output_tokens", "tokens_out");
    let cache_creation_tokens = usage_token_count(
        usage,
        "cache_creation_input_tokens",
        "cache_creation_input_tokens",
    );
    let cache_read_tokens =
        usage_token_count(usage, "cache_read_input_tokens", "cache_read_input_tokens");
    let total_tokens = input_tokens + output_tokens + cache_creation_tokens + cache_read_tokens;
    let detail = format!(
        "tokens: {} in + {} out = {} ({} cache create, {} cache read)",
        input_tokens, output_tokens, total_tokens, cache_creation_tokens, cache_read_tokens
    );
    let metadata = result_metadata(
        payload,
        v,
        token_metadata(
            usage,
            [
                input_tokens,
                output_tokens,
                cache_creation_tokens,
                cache_read_tokens,
            ],
        ),
    );
    let (detail, metadata) = capped_detail_with(&detail, Some(metadata));
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Completion,
        detail,
        metadata,
    })
}

fn token_metadata(usage: &Value, counts: [i64; 4]) -> Value {
    if !has_usage(usage) {
        return json!({});
    }
    let [input, output, creation, read] = counts;
    json!({
        "tokens": input + output + creation + read,
        "input_tokens": input,
        "uncached_input_tokens": input,
        "output_tokens": output,
        "prompt_tokens": input,
        "cache_creation_input_tokens": creation,
        "cache_read_input_tokens": read,
    })
}

fn usage_token_count(usage: &Value, key: &str, alias: &str) -> i64 {
    usage
        .get(key)
        .or_else(|| usage.get(alias))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

fn result_metadata(payload: &Value, envelope: &Value, mut metadata: Value) -> Value {
    let cost_usd = payload
        .get("total_cost_usd")
        .or_else(|| payload.get("cost_usd"))
        .and_then(|value| value.as_f64());
    let model = extract_result_model(payload);
    let session_id = payload
        .get("session_id")
        .or_else(|| envelope.get("session_id"))
        .and_then(|value| value.as_str());
    if let Some(cost) = cost_usd {
        metadata["cost_usd"] = json!(cost);
    }
    if let Some(model) = model {
        metadata["model"] = json!(model);
    }
    if let Some(session_id) = session_id {
        metadata["agent_session_id"] = json!(session_id);
    }
    metadata
}
