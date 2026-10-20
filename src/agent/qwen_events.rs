// Qwen stream event parsing, usage accounting, and event metadata.
// Deps: Qwen-owned helpers, shared event types and provider classification.
use super::super::qwen_support as support;
use super::super::qwen_support::{
    classify_tool_result, extract_error_detail, extract_tool_arguments, extract_tool_name,
};
use crate::types::*;
use chrono::Local;
use serde_json::json;

pub(super) fn parse_stream_event(
    task_id: &TaskId,
    v: &serde_json::Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let event_type = v.get("type")?.as_str()?;
    let (kind, detail, metadata) = match event_type {
        "system" | "init" | "system/init" | "assistant" | "text" | "message" => {
            content_event(event_type, v)?
        }
        "tool_call" | "tool_use" => {
            let name = extract_tool_name(v).unwrap_or("unknown");
            let args = support::truncate(&extract_tool_arguments(v).unwrap_or_default(), 100);
            (EventKind::ToolCall, format!("{name}({args})"), None)
        }
        "tool_result" => {
            let name = extract_tool_name(v).unwrap_or("unknown");
            let output = v.get("output").and_then(|o| o.as_str()).unwrap_or("");
            let (kind, detail) = classify_tool_result(name, output);
            (kind, detail, None)
        }
        "error" => {
            let detail = extract_error_detail(v)?;
            crate::routing::quota::channel::mark_stream_refusal(
                AgentKind::Qwen,
                None,
                &v.to_string(),
            );
            (EventKind::Error, detail, None)
        }
        "result" | "turn_complete" => completion_event(v),
        kind if support::is_skill_or_hook_event(kind) => (
            EventKind::Milestone,
            support::milestone_detail(kind, v),
            None,
        ),
        _ => return None,
    };
    let (detail, metadata) = super::super::truncate::capped_detail_with(&detail, metadata);
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: kind,
        detail,
        metadata,
    })
}

fn completion_metadata(
    usage: Option<Usage>,
    model: Option<&str>,
    session_id: Option<&str>,
) -> Option<serde_json::Value> {
    let mut metadata = json!({});
    let mut has_fields = false;
    if let Some(usage) = usage {
        metadata["tokens"] = json!(usage.total_tokens);
        metadata["input_tokens"] = json!(usage.input_tokens);
        metadata["output_tokens"] = json!(usage.output_tokens);
        metadata["cache_read_input_tokens"] = json!(usage.cache_read_input_tokens);
        metadata["prompt_tokens"] = json!(usage.input_tokens);
        has_fields = true;
    }
    if let Some(model) = model {
        metadata["model"] = json!(model);
        has_fields = true;
    }
    if let Some(session_id) = session_id {
        metadata["agent_session_id"] = json!(session_id);
        has_fields = true;
    }
    has_fields.then_some(metadata)
}

fn base_metadata(model: Option<&str>, session_id: Option<&str>) -> Option<serde_json::Value> {
    match (model, session_id) {
        (None, None) => None,
        _ => {
            let mut metadata = json!({});
            if let Some(model) = model {
                metadata["model"] = json!(model);
            }
            if let Some(session_id) = session_id {
                metadata["agent_session_id"] = json!(session_id);
            }
            Some(metadata)
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Usage {
    input_tokens: i64,
    output_tokens: i64,
    cache_read_input_tokens: i64,
    pub(super) total_tokens: i64,
}

pub(super) fn extract_usage(value: &serde_json::Value) -> Option<Usage> {
    let input_tokens = value
        .pointer("/usage/input_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let output_tokens = value
        .pointer("/usage/output_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let cache_read_input_tokens = value
        .pointer("/usage/cache_read_input_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let total_tokens = value
        .pointer("/usage/total_tokens")
        .and_then(|v| v.as_i64())
        .or_else(|| {
            value
                .pointer("/stats/total_tokens")
                .and_then(|v| v.as_i64())
        })
        .unwrap_or(input_tokens + output_tokens + cache_read_input_tokens);
    (total_tokens > 0).then_some(Usage {
        input_tokens,
        output_tokens,
        cache_read_input_tokens,
        total_tokens,
    })
}

pub(super) fn extract_model(value: &serde_json::Value) -> Option<String> {
    value
        .pointer("/message/model")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("model").and_then(|v| v.as_str()))
        .map(ToOwned::to_owned)
}

fn extract_session_id(value: &serde_json::Value) -> Option<&str> {
    value
        .get("session_id")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("uuid").and_then(|v| v.as_str()))
}

fn extract_assistant_text(value: &serde_json::Value) -> Option<String> {
    support::extract_text_payload(
        value
            .get("message")
            .and_then(|message| message.get("content"))
            .or_else(|| value.get("content")),
    )
}

fn content_event(
    event_type: &str,
    v: &serde_json::Value,
) -> Option<(EventKind, String, Option<serde_json::Value>)> {
    Some(match event_type {
        "system" | "init" | "system/init" => {
            let subtype = v
                .get("subtype")
                .and_then(|value| value.as_str())
                .unwrap_or("init");
            let model = extract_model(v);
            let session_id = extract_session_id(v);
            let detail = model
                .as_deref()
                .map(|value| format!("{subtype}: {value}"))
                .unwrap_or_else(|| subtype.to_string());
            (
                EventKind::Reasoning,
                detail,
                base_metadata(model.as_deref(), session_id),
            )
        }
        "assistant" => {
            let content = extract_assistant_text(v)?;
            let session_id = extract_session_id(v);
            (
                EventKind::Reasoning,
                content,
                base_metadata(None, session_id),
            )
        }
        "text" => {
            let content = v
                .get("content")
                .and_then(|c| c.as_str())
                .or_else(|| v.get("text").and_then(|t| t.as_str()))?;
            (EventKind::Reasoning, content.to_string(), None)
        }
        "message" => {
            if v.get("role").and_then(|r| r.as_str()) != Some("assistant") {
                return None;
            }
            let content = support::extract_text_payload(v.get("content"))?;
            (EventKind::Reasoning, content, None)
        }
        _ => return None,
    })
}

fn completion_event(v: &serde_json::Value) -> (EventKind, String, Option<serde_json::Value>) {
    let usage = extract_usage(v);
    let tokens = usage.map(|value| value.total_tokens);
    let model = extract_model(v);
    let detail = match tokens {
        Some(total) => format!("completed with {total} tokens"),
        None => "completed".to_string(),
    };
    let metadata = completion_metadata(usage, model.as_deref(), extract_session_id(v));
    (EventKind::Completion, detail, metadata)
}
