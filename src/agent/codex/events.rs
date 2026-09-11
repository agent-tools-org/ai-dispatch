// Codex JSONL item, turn, error, and session event parsing.
// Exports parsing helpers to the adapter; depends on serde_json and shared event types.

use super::*;

pub(super) fn parse_item_event(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let event_type = v.get("type")?.as_str()?;
    let item = v.get("item")?;
    let item_type = item.get("type")?.as_str()?;

    match item_type {
        "agent_message" => {
            let text = item
                .get("text")
                .or_else(|| item.get("content"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            if text.is_empty() {
                return None;
            }
            let (detail, metadata) = capped_detail(text);
            Some(TaskEvent {
                task_id: task_id.clone(),
                timestamp: now,
                event_kind: EventKind::Reasoning,
                detail,
                metadata,
            })
        }
        "command_execution" => parse_command_event(task_id, item, event_type, now),
        "file_change" => parse_file_change_event(task_id, item, now),
        "error" => {
            let message = item.get("message").and_then(|m| m.as_str()).unwrap_or("");
            if message.is_empty() {
                return None;
            }
            crate::quota_channel::mark_stream_refusal(AgentKind::Codex, None, &item.to_string());
            let (detail, metadata) = capped_detail(message);
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

fn parse_command_event(
    task_id: &TaskId,
    item: &Value,
    event_type: &str,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let command = item.get("command").and_then(|v| v.as_str()).unwrap_or("");
    if command.is_empty() {
        return None;
    }

    if event_type == "item.started" {
        let (detail, metadata) =
            capped_detail_with(command, Some(json!({ "command": command, "status": "in_progress" })));
        return Some(TaskEvent {
            task_id: task_id.clone(),
            timestamp: now,
            event_kind: classify_command(command),
            detail,
            metadata,
        });
    }

    let exit_code = item.get("exit_code").and_then(|v| v.as_i64());
    if matches!(exit_code, Some(code) if code != 0) {
        return Some(TaskEvent {
            task_id: task_id.clone(),
            timestamp: now,
            event_kind: EventKind::Error,
            detail: format!(
                "command failed ({}) {}",
                exit_code.unwrap_or(-1),
                truncate_text(command, 60)
            ),
            metadata: Some(json!({ "command": command, "exit_code": exit_code })),
        });
    }

    let output = item
        .get("aggregated_output")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let event_kind = classify_output(output)?;
    let (detail, metadata) =
        capped_detail_with(output, Some(json!({ "command": command, "exit_code": exit_code })));
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind,
        detail,
        metadata,
    })
}

pub(super) fn parse_turn_completed(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let usage = v.get("usage")?;
    let input_tokens = usage
        .get("input_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let cached_input_tokens = usage
        .get("cached_input_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("output_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
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
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Completion,
        detail,
        metadata: Some(completion_metadata(
            total_tokens,
            input_tokens,
            output_tokens,
            cached_input_tokens,
            extract_model(v),
            cost_usd,
        )),
    })
}

pub(super) fn parse_error_event(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let detail = v
        .get("message")
        .or_else(|| v.pointer("/error/message"))
        .and_then(|value| value.as_str())
        .filter(|message| !message.is_empty())?;

    crate::quota_channel::mark_stream_refusal(AgentKind::Codex, None, &v.to_string());

    let (detail, metadata) = capped_detail(detail);
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Error,
        detail,
        metadata,
    })
}

pub(super) fn parse_thread_started(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let thread_id = v.get("thread_id")?.as_str()?;
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Milestone,
        detail: format!("session {}", thread_id),
        metadata: Some(json!({ "agent_session_id": thread_id })),
    })
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
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::FileWrite,
        detail,
        metadata,
    })
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
        ("output_tokens".to_string(), json!(output_tokens)),
        (
            "cached_input_tokens".to_string(),
            json!(cached_input_tokens),
        ),
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
