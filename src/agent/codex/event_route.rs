// Route-aware Codex stream parsing shared by default and routed adapters.
// Passes the persisted marker identity to refusal detection.
use super::*;

pub(super) fn parse_event(task_id: &TaskId, line: &str, route: Option<&str>) -> Option<TaskEvent> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let now = Local::now();

    let event_type = v.get("type")?.as_str()?;
    match event_type {
        "item.started" | "item.completed" => parse_item_event(task_id, &v, now, route),
        "codex.reasoning" => {
            let mut item = v.clone();
            item["item"]["type"] = json!("agent_message");
            parse_item_event(task_id, &item, now, route)
        }
        "turn.completed" => parse_turn_completed(task_id, &v, now),
        "thread.started" => parse_thread_started(task_id, &v, now),
        "error" | "turn.failed" => parse_error_event(task_id, &v, now, route),
        _ => None,
    }
}
