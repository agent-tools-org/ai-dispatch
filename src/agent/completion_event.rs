// Completion events for buffered and PTY tasks.
// Exports: make_completion_event. Deps: task types, chrono, serde_json.

use crate::types::{CompletionInfo, EventKind, TaskEvent, TaskId};
use serde_json::json;

pub fn make_completion_event(task_id: &TaskId, info: &CompletionInfo) -> TaskEvent {
    let detail = match info.tokens {
        Some(t) => format!("completed with {} tokens", t),
        None => "completed".to_string(),
    };
    let metadata = info.tokens.map(|tokens| json!({ "tokens": tokens }));
    TaskEvent::new(task_id.clone(), EventKind::Completion, detail).with_metadata(metadata)
}
