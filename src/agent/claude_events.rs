// Claude stream-json parsing helpers shared by the Claude adapter and tests.
// Exports parse_event_line() plus internal helpers for assistant, tool, and result events.
// Depends on serde_json for decoding and capped_detail for concise event details.
#[path = "claude_model.rs"]
mod model;
use model::{extract_result_model, normalize_model};
#[path = "claude_usage.rs"]
mod usage;
use usage::parse_result_event;
use chrono::Local;
use serde_json::{Value, json};
use super::truncate::{capped_detail, capped_detail_with};
use crate::types::*;

pub(crate) fn parse_event_line(task_id: &TaskId, line: &str) -> Option<TaskEvent> {
    parse_event_line_for_route(task_id, line, None)
}

pub(crate) fn parse_event_line_for_route(
    task_id: &TaskId, line: &str, route: Option<&str>,
) -> Option<TaskEvent> {
    let cleaned = crate::runtime::watcher::strip_terminal_escapes(line);
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(trimmed).ok()?;
    let now = Local::now();
    match v.get("type").and_then(|value| value.as_str())? {
        "assistant" => parse_assistant_event(task_id, &v, now),
        "tool_use" => parse_tool_use_event(task_id, &v, now),
        "result" => parse_result_event(task_id, &v, now),
        "system" => parse_system_event(task_id, &v, now).map(|mut event| {
            if route.is_some() && v["subtype"] == "init" && v["apiKeySource"] == "apiKeyHelper" {
                let metadata = event.metadata.get_or_insert_with(|| json!({}));
                metadata["api_route_init_verified"] = json!(true);
            }
            event
        }),
        "user" => parse_user_event(task_id, &v, now),
        "error" => parse_error_event(task_id, &v, now, route),
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
    let (detail, metadata) = capped_detail_with(text, metadata);
    Some(TaskEvent::at(task_id.clone(), EventKind::Reasoning, detail, now).with_metadata(metadata))
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
    let text = command
        .map(|value| format!("{name}: {value}"))
        .unwrap_or_else(|| name.to_string());
    let (detail, metadata) = capped_detail(&text);
    Some(TaskEvent::at(task_id.clone(), EventKind::ToolCall, detail, now).with_metadata(metadata))
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
        let (detail, metadata) = capped_detail_with(&detail, metadata);
        return Some(TaskEvent::at(task_id.clone(), EventKind::Reasoning, detail, now).with_metadata(metadata));
    }
    if subtype == "hook_response" && v.get("outcome").and_then(|value| value.as_str()) == Some("error")
    {
        let text = v
            .get("stderr")
            .or_else(|| v.get("output"))
            .and_then(|value| value.as_str())?;
        let (detail, metadata) = capped_detail(text);
        return Some(TaskEvent::at(task_id.clone(), EventKind::Error, detail, now).with_metadata(metadata));
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
    // No marking here. `is_error` on a tool_result block is the *tool* failing —
    // a command that exited non-zero, a rejected edit, a file that was not
    // there — and its text is whatever the model asked for. This wrote
    // `~/.aid/rate-limit-claude` on 2026-08-07 off an agent's own message, and a
    // provider that was serving the whole time was held on opencode's needle.
    let (detail, metadata) = capped_detail(detail);
    Some(TaskEvent::at(task_id.clone(), EventKind::Error, detail, now).with_metadata(metadata))
}

fn parse_error_event(
    task_id: &TaskId, v: &Value, now: chrono::DateTime<Local>, route: Option<&str>,
) -> Option<TaskEvent> {
    let detail = v.get("message").or_else(|| v.pointer("/error/message")).and_then(|value| value.as_str()).filter(|msg| !msg.is_empty())?;
    if route.is_none() {
        crate::routing::quota::channel::mark_stream_refusal(AgentKind::Claude, None, &v.to_string());
    }
    let (detail, metadata) = capped_detail(detail);
    Some(TaskEvent::at(task_id.clone(), EventKind::Error, detail, now).with_metadata(metadata))
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

#[cfg(test)]
#[path = "claude_events_tests.rs"]
mod tests;
