// Codex JSONL item, turn, error, and session event parsing.
// Exports parsing helpers to the adapter; depends on serde_json and shared event types.

use super::*;
#[path = "events/commands.rs"]
mod commands;
use commands::parse_command_event;

pub(super) fn parse_item_event(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
    route: Option<&str>,
) -> Option<TaskEvent> {
    let event_type = v.get("type")?.as_str()?;
    let item = v.get("item")?;
    let item_type = item.get("type")?.as_str()?;

    match item_type {
        "agent_message" => parse_agent_message(task_id, item, event_type, now),
        "command_execution" => parse_command_event(task_id, item, event_type, now),
        "file_change" => parse_file_change_event(task_id, item, now),
        "error" => {
            let message = item.get("message").and_then(|m| m.as_str()).unwrap_or("");
            if message.is_empty() {
                return None;
            }
            crate::routing::quota::channel::mark_stream_refusal(AgentKind::Codex, route, &item.to_string());
            let (detail, metadata) = capped_detail(message);
            Some(TaskEvent::at(task_id.clone(), EventKind::Error, detail, now).with_metadata(metadata))
        }
        _ => None,
    }
}

fn parse_agent_message(
    task_id: &TaskId,
    item: &Value,
    event_type: &str,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let text = item
        .get("text")
        .or_else(|| item.get("content"))
        .and_then(|t| t.as_str())
        .unwrap_or("");
    if text.is_empty() {
        return None;
    }
    if event_type == "item.completed"
        && matches!(
            item.get("phase").and_then(Value::as_str),
            None | Some("final_answer")
        )
        && let Some(marker) = text
            .lines()
            .map(str::trim_start)
            .find(|line| line.starts_with("NO_CHANGES_NEEDED"))
    {
        return Some(TaskEvent::at(task_id.clone(), EventKind::NoOp, extract_noop_reason(marker), now));
    }
    let (detail, metadata) = capped_detail(text);
    Some(TaskEvent::at(task_id.clone(), EventKind::Reasoning, detail, now).with_metadata(metadata))
}

pub(super) fn parse_turn_completed(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let usage = v.get("usage")?;
    let input_tokens = usage.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
    let cached_input_tokens = usage
        .get("cached_input_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let output_tokens = usage.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
    let total_tokens = input_tokens + output_tokens;
    let detail = if cached_input_tokens > 0 {
        format!(
            "tokens: {} in + {} out = {} ({} cached)",
            input_tokens, output_tokens, total_tokens, cached_input_tokens
        )
    } else {
        format!(
            "tokens: {} in + {} out = {}",
            input_tokens, output_tokens, total_tokens
        )
    };

    let cost_usd = v.get("cost_usd").and_then(|c| c.as_f64());
    Some(TaskEvent::at(task_id.clone(), EventKind::Completion, detail, now)
        .with_metadata(Some(completion_metadata(
            total_tokens,
            input_tokens,
            output_tokens,
            cached_input_tokens,
            extract_model(v),
            cost_usd,
        ))))
}

pub(super) fn parse_error_event(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
    route: Option<&str>,
) -> Option<TaskEvent> {
    let detail = v
        .get("message")
        .or_else(|| v.pointer("/error/message"))
        .and_then(|value| value.as_str())
        .filter(|message| !message.is_empty())?;

    crate::routing::quota::channel::mark_stream_refusal(AgentKind::Codex, route, &v.to_string());

    let (detail, metadata) = capped_detail(detail);
    Some(TaskEvent::at(task_id.clone(), EventKind::Error, detail, now).with_metadata(metadata))
}

pub(super) fn parse_thread_started(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let thread_id = v.get("thread_id")?.as_str()?;
    let mut metadata = json!({ "agent_session_id": thread_id });
    if let Some(model) = v.get("model").and_then(Value::as_str) {
        metadata["model"] = json!(model);
    }
    Some(TaskEvent::at(task_id.clone(), EventKind::Milestone, format!("session {}", thread_id), now)
        .with_metadata(Some(metadata)))
}

fn parse_file_change_event(
    task_id: &TaskId,
    item: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let changes = item.get("changes")?.as_array()?;
    let paths: Vec<&str> = changes
        .iter()
        .filter_map(|c| c.get("path").and_then(|p| p.as_str()))
        .collect();
    if paths.is_empty() {
        return None;
    }
    let text = if paths.len() == 1 {
        paths[0].to_string()
    } else {
        format!("{} files changed", paths.len())
    };
    let (detail, metadata) = capped_detail_with(&text, Some(json!({ "files": paths })));
    Some(TaskEvent::at(task_id.clone(), EventKind::FileWrite, detail, now).with_metadata(metadata))
}

fn completion_metadata(
    total_tokens: i64,
    input_tokens: i64,
    output_tokens: i64,
    cached_input_tokens: i64,
    model: Option<String>,
    cost_usd: Option<f64>,
) -> Value {
    let mut map = Map::from_iter([
        ("tokens".to_string(), json!(total_tokens)),
        ("input_tokens".to_string(), json!(input_tokens)),
        (
            "uncached_input_tokens".to_string(),
            json!(input_tokens.saturating_sub(cached_input_tokens)),
        ),
        ("output_tokens".to_string(), json!(output_tokens)),
        ("cached_input_tokens".to_string(), json!(cached_input_tokens)),
    ]);
    if let Some(value) = model {
        map.insert("model".to_string(), json!(value));
    }
    if let Some(cost) = cost_usd {
        map.insert("cost_usd".to_string(), json!(cost));
    }
    Value::Object(map)
}

fn extract_model(v: &Value) -> Option<String> {
    [
        "/model",
        "/assistant/model",
        "/session/model",
        "/turn/model",
        "/usage/model",
        "/item/model",
    ]
    .iter()
    .find_map(|pointer| v.pointer(pointer).and_then(|value| value.as_str()))
    .map(ToOwned::to_owned)
}
