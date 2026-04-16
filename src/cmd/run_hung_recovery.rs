// Hung recovery helpers for idle-timeout failures.
// Exports retry policy, feedback text, and hung event metadata parsing.
// Deps: serde_json, crate::store, crate::types.

use anyhow::Result;
use chrono::Local;
use serde_json::json;

use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent, TaskId, TaskStatus};

pub const MAX_HUNG_RETRIES: u32 = 2;

const MIN_PROGRESS_EVENTS: i64 = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HungContext {
    pub(crate) hung_duration_secs: u64,
    pub(crate) event_count: u32,
    pub(crate) last_event_detail: Option<String>,
}

pub fn build_hung_retry_feedback(task: &Task, hung_duration_secs: u64) -> String {
    let detail = task
        .resolved_prompt
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("last progress detail unavailable");
    let worktree_note = if task.worktree_path.is_some() {
        " Partial work is already committed in the worktree. Review what's there and continue."
    } else {
        ""
    };
    format!(
        "Previous attempt hung after {hung_duration_secs} seconds of no output. The task was working on: {detail}. Continue from where you stopped. If you're stuck on the same approach, try a different strategy.{worktree_note}"
    )
}

pub fn should_auto_retry_hung(task: &Task, retry_count: u32) -> bool {
    task.status == TaskStatus::Failed
        && retry_count < MAX_HUNG_RETRIES
        && task.prompt_tokens.unwrap_or_default() >= MIN_PROGRESS_EVENTS
}

pub(crate) fn insert_hung_detected_events(
    store: &Store,
    task_id: &TaskId,
    hung_duration_secs: u64,
    event_count: u32,
    last_event_detail: Option<&str>,
) -> Result<()> {
    let metadata = json!({
        "hung_recovery_eligible": true,
        "hung_duration_secs": hung_duration_secs,
        "event_count": event_count,
        "last_event_detail": last_event_detail,
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

pub(crate) fn with_hung_context(task: &Task, context: &HungContext) -> Task {
    let mut enriched = task.clone();
    enriched.resolved_prompt = context.last_event_detail.clone();
    enriched.prompt_tokens = Some(i64::from(context.event_count));
    enriched
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use crate::types::{AgentKind, VerifyStatus};

    fn task() -> Task {
        Task {
            id: TaskId("t-hung".to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "prompt".to_string(),
            resolved_prompt: None,
            category: None,
            status: TaskStatus::Failed,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            start_sha: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            pending_reason: None,
            read_only: false,
            budget: false,
            audit_verdict: None,
            audit_report_path: None,
            delivery_assessment: None,
        }
    }

    #[test]
    fn should_auto_retry_hung_for_progressing_task() {
        let mut task = task();
        task.prompt_tokens = Some(6);
        assert!(should_auto_retry_hung(&task, 0));
    }

    #[test]
    fn should_not_auto_retry_immediate_failures() {
        let mut task = task();
        task.prompt_tokens = Some(5);
        assert!(!should_auto_retry_hung(&task, 0));
    }

    #[test]
    fn build_hung_retry_feedback_includes_last_event_detail() {
        let mut task = task();
        task.resolved_prompt = Some("updating watcher timeout handling".to_string());
        let feedback = build_hung_retry_feedback(&task, 300);
        assert!(feedback.contains("updating watcher timeout handling"));
    }

    #[test]
    fn should_not_retry_past_hung_limit() {
        let mut task = task();
        task.prompt_tokens = Some(9);
        assert!(!should_auto_retry_hung(&task, MAX_HUNG_RETRIES));
    }
}
