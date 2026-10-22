// Codex command start, failure, and output events.
// Uses the parent event parser types and output classifiers.

use super::*;

pub(super) fn parse_command_event(
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
        return command_started(task_id, command, now);
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
    let (detail, metadata) = capped_detail_with(
        output,
        Some(json!({ "command": command, "exit_code": exit_code })),
    );
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind,
        detail,
        metadata,
    })
}

fn command_started(task_id: &TaskId, command: &str, now: chrono::DateTime<Local>) -> Option<TaskEvent> {
    let (detail, metadata) = capped_detail_with(
        command,
        Some(json!({ "command": command, "status": "in_progress" })),
    );
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: classify_command(command),
        detail,
        metadata,
    })
}
