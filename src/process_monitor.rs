// Command-neutral hung-task event recording and metadata parsing.
// Exports hung-detected/retry event insertion and HungContext extraction.
// Deps: serde_json, crate::store, crate::types.

use anyhow::Result;
use chrono::Local;
use serde_json::json;

use crate::store::Store;
use crate::types::{EventKind, TaskEvent, TaskId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HungContext {
    pub(crate) hung_duration_secs: u64,
    pub(crate) event_count: u32,
    pub(crate) last_event_detail: Option<String>,
    pub(crate) transient: bool,
}

pub(crate) fn insert_hung_detected_events(
    store: &Store,
    task_id: &TaskId,
    hung_duration_secs: u64,
    event_count: u32,
    last_event_detail: Option<&str>,
    transient: bool,
) -> Result<()> {
    let metadata = json!({
        "hung_recovery_eligible": true,
        "hung_duration_secs": hung_duration_secs,
        "event_count": event_count,
        "last_event_detail": last_event_detail,
        "transient": transient,
    });
    store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Milestone,
        detail: "hung_detected".to_string(),
        metadata: Some(metadata.clone()),
    })?;
    store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Error,
        detail: format!("Agent hung: no output for {hung_duration_secs} seconds"),
        metadata: Some(metadata),
    })?;
    Ok(())
}

pub(crate) fn insert_hung_retry_event(store: &Store, task_id: &TaskId) -> Result<()> {
    store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Error,
        detail: "HUNG → retry".to_string(),
        metadata: Some(json!({ "hung_auto_retried": true })),
    })?;
    Ok(())
}

pub(crate) fn hung_context(events: &[TaskEvent]) -> Option<HungContext> {
    events.iter().rev().find_map(|event| {
        let metadata = event.metadata.as_ref()?;
        if !metadata
            .get("hung_recovery_eligible")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            return None;
        }
        Some(HungContext {
            hung_duration_secs: metadata
                .get("hung_duration_secs")
                .and_then(|value| value.as_u64())
                .unwrap_or_default(),
            event_count: metadata
                .get("event_count")
                .and_then(|value| value.as_u64())
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or_default(),
            last_event_detail: metadata
                .get("last_event_detail")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            transient: metadata
                .get("transient")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
        })
    })
}

pub(crate) fn was_auto_retried_after_hang(events: &[TaskEvent]) -> bool {
    events.iter().any(|event| {
        event
            .metadata
            .as_ref()
            .and_then(|value| value.get("hung_auto_retried"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(metadata: serde_json::Value) -> TaskEvent {
        TaskEvent {
            task_id: TaskId("t-hung".to_string()),
            timestamp: Local::now(),
            event_kind: EventKind::Milestone,
            detail: "hung_detected".to_string(),
            metadata: Some(metadata),
        }
    }

    #[test]
    fn hung_context_extracts_metadata_fields() {
        let events = vec![event(json!({
            "hung_recovery_eligible": true,
            "hung_duration_secs": 300,
            "event_count": 7,
            "last_event_detail": "compiling watcher",
            "transient": true,
        }))];
        let context = hung_context(&events).expect("context");
        assert_eq!(context.hung_duration_secs, 300);
        assert_eq!(context.event_count, 7);
        assert_eq!(context.last_event_detail.as_deref(), Some("compiling watcher"));
        assert!(context.transient);
    }

    #[test]
    fn hung_context_ignores_ineligible_events() {
        let events = vec![event(json!({ "hung_recovery_eligible": false }))];
        assert!(hung_context(&events).is_none());
    }

    #[test]
    fn was_auto_retried_after_hang_detects_retry_marker() {
        let events = vec![event(json!({ "hung_auto_retried": true }))];
        assert!(was_auto_retried_after_hang(&events));
        assert!(!was_auto_retried_after_hang(&[]));
    }
}
