// Command Code NDJSON event parsing: stream events, result envelope, usage.
// Exports: parse_line, parse_result_event, commandcode_result_failed.
// Deps: super::truncate, crate::types, serde_json, chrono.

use chrono::Local;
use serde_json::{json, Value};

use crate::agent::truncate::{capped_detail, capped_detail_with};
use crate::types::*;

pub(crate) fn parse_line(task_id: &TaskId, value: &Value, now: chrono::DateTime<Local>) -> Option<TaskEvent> {
    match value.get("type").and_then(Value::as_str)? {
        "event" => parse_stream_event(task_id, value.get("event")?, now),
        "result" => parse_result_event(task_id, value, now),
        "error" => {
            let detail = value
                .get("message")
                .or_else(|| value.pointer("/error/message"))
                .and_then(Value::as_str)
                .unwrap_or("commandcode error");
            let (detail, metadata) = capped_detail(detail);
            Some(TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: EventKind::Error,
                detail,
                metadata,
            })
        }
        _ => None,
    }
}

pub(crate) fn parse_stream_event(
    task_id: &TaskId,
    event: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    match event.get("type").and_then(Value::as_str)? {
        "run_start" => {
            let session_id = event.get("sessionId")?.as_str()?;
            Some(TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: EventKind::Milestone,
                detail: format!("session {session_id}"),
                metadata: Some(json!({ "agent_session_id": session_id })),
            })
        }
        "model_request_start" => {
            let model = event.get("model")?.as_str()?;
            let (detail, metadata) =
                capped_detail_with(&format!("model {model}"), Some(json!({ "model": model })));
            Some(TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: EventKind::Milestone,
                detail,
                metadata,
            })
        }
        "thinking_end" => {
            let text = event.get("text").and_then(Value::as_str).unwrap_or("");
            (!text.is_empty()).then(|| {
                let (detail, metadata) = capped_detail(text);
                TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: now,
                    event_kind: EventKind::Reasoning,
                    detail,
                    metadata,
                }
            })
        }
        "tool_queued" | "tool_running" | "tool_completed" => {
            let tool = event
                .get("toolName")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let detail = format!("{tool}: {}", event.get("type").and_then(Value::as_str)?);
            let (detail, metadata) = capped_detail_with(&detail, Some(json!({ "tool": tool })));
            Some(TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: EventKind::ToolCall,
                detail,
                metadata,
            })
        }
        "turn_end" => usage_event(task_id, event.get("usage")?, now),
        _ => None,
    }
}

pub(crate) fn parse_result_event(
    task_id: &TaskId,
    value: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let subtype = value.get("subtype").and_then(Value::as_str).unwrap_or("unknown");
    let stop_reason = value
        .get("stopReason")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let usage = value.get("usage");
    let metadata = usage.and_then(usage_metadata).map(|mut metadata| {
        metadata["subtype"] = json!(subtype);
        metadata["stop_reason"] = json!(stop_reason);
        metadata
    });
    if commandcode_result_failed(value) {
        let detail = format!("Command Code stopped with {subtype} ({stop_reason})");
        let (detail, metadata) = capped_detail_with(&detail, metadata);
        Some(TaskEvent {
            task_id: task_id.clone(),
            timestamp: now,
            event_kind: EventKind::Error,
            detail,
            metadata,
        })
    } else {
        let detail = usage
            .and_then(usage_detail)
            .unwrap_or_else(|| "completed".to_string());
        let (detail, metadata) = capped_detail_with(&detail, metadata);
        Some(TaskEvent {
            task_id: task_id.clone(),
            timestamp: now,
            event_kind: EventKind::Completion,
            detail,
            metadata,
        })
    }
}

pub(crate) fn usage_event(task_id: &TaskId, usage: &Value, now: chrono::DateTime<Local>) -> Option<TaskEvent> {
    let detail = usage_detail(usage)?;
    let metadata = usage_metadata(usage);
    let (detail, metadata) = capped_detail_with(&detail, metadata);
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Completion,
        detail,
        metadata,
    })
}

pub(crate) fn usage_detail(usage: &Value) -> Option<String> {
    let input = usage.get("inputTokens").and_then(Value::as_i64)?;
    let output = usage.get("outputTokens").and_then(Value::as_i64)?;
    let cached = usage
        .get("cacheReadTokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let total = input + output;
    Some(if cached > 0 {
        format!("tokens: {input} in + {output} out = {total} ({cached} cached)")
    } else {
        format!("tokens: {input} in + {output} out = {total}")
    })
}

pub(crate) fn usage_metadata(usage: &Value) -> Option<Value> {
    let input = usage.get("inputTokens").and_then(Value::as_i64)?;
    let output = usage.get("outputTokens").and_then(Value::as_i64)?;
    let cached = usage
        .get("cacheReadTokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let written = usage
        .get("cacheWriteTokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    Some(json!({
        "tokens": input + output,
        "input_tokens": input,
        "output_tokens": output,
        "cached_input_tokens": cached,
        "cache_write_tokens": written,
    }))
}

pub(crate) fn commandcode_result_failed(value: &Value) -> bool {
    value.get("subtype").and_then(Value::as_str) != Some("success")
        || value.get("stopReason").and_then(Value::as_str) != Some("end_turn")
}
