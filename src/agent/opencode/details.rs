// OpenCode event detail and metadata decoding.
// Uses the adapter classification and quota helpers.

use super::*;

pub(super) fn event_detail(
    signature_kind: AgentKind,
    marker_kind: AgentKind,
    custom_name: Option<&str>,
    v: &serde_json::Value,
    event_type: &str,
) -> Option<(String, Option<serde_json::Value>)> {
    Some(match event_type {
        "tool_call" | "function_call" => {
            let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
            let args = v.get("arguments").and_then(|a| a.as_str()).unwrap_or("");
            (format!("{name}: {args}"), None)
        }
        "message" => {
            let detail = v
                .get("content")
                .or(v.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            (detail, Some(json!({ "assistant_output": true })))
        }
        "text" => (
            v.pointer("/part/text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string(),
            Some(json!({ "assistant_output": true })),
        ),
        "auto_compact" | "git_snapshot" => (milestone_detail(v, event_type), None),
        "step_start" => return None,
        "step_finish" => step_finish_detail(v, event_type)?,
        "completion" | "done" => {
            let tokens = v.get("tokens").and_then(|t| t.as_i64());
            (
                tokens
                    .map(|t| format!("completed with {t} tokens"))
                    .unwrap_or_else(|| "completed".to_string()),
                tokens.map(|value| json!({ "tokens": value })),
            )
        }
        "error" => error_event_detail(signature_kind, marker_kind, custom_name, v),
        _ => return None,
    })
}

fn error_event_detail(
    signature_kind: AgentKind,
    marker_kind: AgentKind,
    custom_name: Option<&str>,
    v: &serde_json::Value,
) -> (String, Option<serde_json::Value>) {
    let detail = error_detail(v).unwrap_or("unknown error");
    // Watcher owns built-in OpenCode (has the model); overlays write here.
    if marker_kind != AgentKind::OpenCode {
        if let Some(message) = rate_limit::refusal_on_channel(
            &v.to_string(),
            signature_kind,
            crate::routing::quota::channel::Channel::CliStream,
        ) {
            rate_limit::mark_rate_limited_for_signature(&marker_kind, custom_name, signature_kind, &message);
        }
    }
    let auth = crate::routing::auth_marker::auth_failure_line(
        &v.to_string(),
        marker_kind,
        crate::routing::quota::channel::Channel::CliStream,
    );
    (
        detail.to_string(),
        auth.map(|message| json!({ "opencode_auth_failure": message })),
    )
}

fn step_finish_detail(
    v: &serde_json::Value,
    event_type: &str,
) -> Option<(String, Option<serde_json::Value>)> {
    let total = v.pointer("/part/tokens/total").and_then(|t| t.as_i64())?;
    let input = v.pointer("/part/tokens/input").and_then(|t| t.as_i64())?;
    let output = v.pointer("/part/tokens/output").and_then(|t| t.as_i64())?;
    let cost = v.pointer("/part/cost").and_then(|c| c.as_f64())?;
    Some((
        format!("tokens: {} in + {} out = {}", input, output, total),
        Some(json!({
            "tokens": total,
            "input_tokens": input,
            "output_tokens": output,
            "cost_usd": cost,
            "usage_is_delta": event_type == "step_finish",
        })),
    ))
}

pub(super) fn session_metadata(
    metadata: Option<serde_json::Value>,
    session_id: Option<&str>,
) -> Option<serde_json::Value> {
    if let Some(sid) = session_id {
        match metadata {
            Some(mut m) => {
                if let Some(obj) = m.as_object_mut() {
                    obj.insert("agent_session_id".to_string(), json!(sid));
                }
                Some(m)
            }
            None => Some(json!({ "agent_session_id": sid })),
        }
    } else {
        metadata
    }
}
