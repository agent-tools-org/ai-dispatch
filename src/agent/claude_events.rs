// Claude stream-json parsing helpers shared by the Claude adapter and tests.
// Exports parse_event_line() plus internal helpers for assistant, tool, and result events.
// Depends on serde_json for decoding and truncate_text for concise event details.

use chrono::Local;
use serde_json::{Value, json};

use super::truncate::truncate_text;
use crate::types::*;

pub(crate) fn parse_event_line(task_id: &TaskId, line: &str) -> Option<TaskEvent> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(trimmed).ok()?;
    let now = Local::now();
    if trimmed.contains("NO_CHANGES_NEEDED") {
        return Some(TaskEvent {
            task_id: task_id.clone(),
            timestamp: now,
            event_kind: EventKind::NoOp,
            detail: extract_noop_reason(trimmed),
            metadata: None,
        });
    }
    match v.get("type").and_then(|value| value.as_str())? {
        "assistant" => parse_assistant_event(task_id, &v, now),
        "tool_use" => parse_tool_use_event(task_id, &v, now),
        "result" => parse_result_event(task_id, &v, now),
        "system" => parse_system_event(task_id, &v, now),
        "user" => parse_user_event(task_id, &v, now),
        _ => None,
    }
}

fn parse_assistant_event(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let message = v.get("message")?;
    let content = message.get("content")?.as_array()?;
    if let Some(item) = content
        .iter()
        .find(|item| item.get("type").and_then(|value| value.as_str()) == Some("tool_use"))
    {
        return build_tool_event(task_id, item, now);
    }
    let text = content.iter().find_map(|item| {
        if item.get("type").and_then(|value| value.as_str()) == Some("text") {
            item.get("text").and_then(|value| value.as_str())
        } else {
            None
        }
    })?;
    let metadata = base_metadata(
        message.get("model").and_then(|value| value.as_str()),
        v.get("session_id").and_then(|value| value.as_str()),
    );
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Reasoning,
        detail: truncate_text(text, 80),
        metadata,
    })
}

fn parse_tool_use_event(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let tool = v.get("tool").unwrap_or(v);
    build_tool_event(task_id, tool, now)
}

fn build_tool_event(
    task_id: &TaskId,
    tool: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let name = tool.get("name").and_then(|value| value.as_str()).unwrap_or("tool");
    let command = tool
        .pointer("/input/command")
        .and_then(|value| value.as_str())
        .or_else(|| tool.pointer("/input/description").and_then(|value| value.as_str()));
    let detail = command
        .map(|value| format!("{name}: {value}"))
        .unwrap_or_else(|| name.to_string());
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::ToolCall,
        detail: truncate_text(&detail, 80),
        metadata: None,
    })
}

fn parse_result_event(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let payload = v.get("result").filter(|value| value.is_object()).unwrap_or(v);
    let usage = payload.get("usage").unwrap_or(payload);
    let input_tokens = usage
        .get("input_tokens")
        .or_else(|| usage.get("tokens_in"))
        .and_then(|value| value.as_i64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("output_tokens")
        .or_else(|| usage.get("tokens_out"))
        .and_then(|value| value.as_i64())
        .unwrap_or(0);
    let cache_creation_tokens = usage
        .get("cache_creation_input_tokens")
        .and_then(|value| value.as_i64())
        .unwrap_or(0);
    let cache_read_tokens = usage
        .get("cache_read_input_tokens")
        .and_then(|value| value.as_i64())
        .unwrap_or(0);
    let total_tokens = input_tokens + output_tokens + cache_creation_tokens + cache_read_tokens;
    let detail = format!(
        "tokens: {} in + {} out = {} ({} cache create, {} cache read)",
        input_tokens,
        output_tokens,
        total_tokens,
        cache_creation_tokens,
        cache_read_tokens
    );
    let cost_usd = payload
        .get("total_cost_usd")
        .or_else(|| payload.get("cost_usd"))
        .and_then(|value| value.as_f64());
    let model = extract_result_model(payload);
    let session_id = payload
        .get("session_id")
        .or_else(|| v.get("session_id"))
        .and_then(|value| value.as_str());
    let mut metadata = json!({
        "tokens": total_tokens,
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "prompt_tokens": input_tokens,
        "cache_creation_input_tokens": cache_creation_tokens,
        "cache_read_input_tokens": cache_read_tokens,
    });
    if let Some(cost) = cost_usd {
        metadata["cost_usd"] = json!(cost);
    }
    if let Some(model) = model {
        metadata["model"] = json!(model);
    }
    if let Some(session_id) = session_id {
        metadata["agent_session_id"] = json!(session_id);
    }
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Completion,
        detail: truncate_text(&detail, 80),
        metadata: Some(metadata),
    })
}

fn parse_system_event(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let subtype = v.get("subtype").and_then(|value| value.as_str())?;
    if subtype == "init" {
        let detail = v
            .get("model")
            .and_then(|value| value.as_str())
            .map(|model| format!("init: {model}"))
            .unwrap_or_else(|| "init".to_string());
        let metadata = base_metadata(
            v.get("model").and_then(|value| value.as_str()),
            v.get("session_id").and_then(|value| value.as_str()),
        );
        return Some(TaskEvent {
            task_id: task_id.clone(),
            timestamp: now,
            event_kind: EventKind::Reasoning,
            detail: truncate_text(&detail, 80),
            metadata,
        });
    }
    if subtype == "hook_response" && v.get("outcome").and_then(|value| value.as_str()) == Some("error")
    {
        let detail = v
            .get("stderr")
            .or_else(|| v.get("output"))
            .and_then(|value| value.as_str())?;
        return Some(TaskEvent {
            task_id: task_id.clone(),
            timestamp: now,
            event_kind: EventKind::Error,
            detail: truncate_text(detail, 80),
            metadata: None,
        });
    }
    None
}

fn parse_user_event(task_id: &TaskId, v: &Value, now: chrono::DateTime<Local>) -> Option<TaskEvent> {
    let detail = v
        .get("tool_use_result")
        .and_then(|value| value.as_str())
        .or_else(|| {
            v.pointer("/message/content/0/content")
                .and_then(|value| value.as_str())
        })?;
    let is_error = v
        .pointer("/message/content/0/is_error")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if !is_error {
        return None;
    }
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Error,
        detail: truncate_text(detail, 80),
        metadata: None,
    })
}

fn base_metadata(model: Option<&str>, session_id: Option<&str>) -> Option<Value> {
    match (model, session_id) {
        (None, None) => None,
        _ => {
            let mut metadata = json!({});
            if let Some(model) = model {
                metadata["model"] = json!(normalize_model(model));
            }
            if let Some(session_id) = session_id {
                metadata["agent_session_id"] = json!(session_id);
            }
            Some(metadata)
        }
    }
}

fn extract_result_model(payload: &Value) -> Option<String> {
    payload
        .pointer("/modelUsage")
        .and_then(|value| value.as_object())
        .and_then(|models| models.keys().next())
        .map(|name| normalize_model(name))
        .or_else(|| {
            payload
                .get("model")
                .and_then(|value| value.as_str())
                .map(normalize_model)
        })
}

fn normalize_model(model: &str) -> String {
    model.split('[').next().unwrap_or(model).to_string()
}

fn extract_noop_reason(line: &str) -> String {
    if let Some(pos) = line.find("NO_CHANGES_NEEDED:") {
        format!("NO_CHANGES_NEEDED: {}", line[pos + 18..].trim().trim_matches('"'))
    } else {
        "NO_CHANGES_NEEDED".to_string()
    }
}
