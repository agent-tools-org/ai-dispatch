// Completion summary events shared by PTY and piped agent runners.
// Preserves terminal labels, duration, token counts and formatted cost.
use super::*;

pub(crate) fn event(
    agent: &dyn Agent,
    task_id: &TaskId,
    store: &Store,
    duration_ms: i64,
    info: &CompletionInfo,
    cost_usd: Option<f64>,
) -> TaskEvent {
    TaskEvent {
        task_id: task_id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: if info.status == crate::types::TaskStatus::Done {
            crate::types::EventKind::Completion
        } else {
            crate::types::EventKind::Error
        },
        detail: format!(
            "{} ({}{}{})",
            crate::routing::capacity::terminal_label(store, task_id, agent.kind(), info.status),
            format_duration(duration_ms),
            info.tokens
                .map(|t| format!(", {} tokens", t))
                .unwrap_or_default(),
            cost_usd
                .map(|c| format!(", {}", cost::format_cost(Some(c))))
                .unwrap_or_default(),
        ),
        metadata: None,
    }
}
