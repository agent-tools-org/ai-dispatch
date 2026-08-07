// Pins failure attribution for the result-file missing-report guard.
// Covers: suppress when a kill cause is already recorded; still flag a real miss.
// Deps: missing_report, Store, ResultDelivery, task fixtures.

use super::run_lifecycle::record_missing_report;
use super::run_prompt::ResultDelivery;
use crate::store::Store;
use crate::types::{
    AgentKind, DeliveryAssessment, EventKind, Task, TaskEvent, TaskId, TaskStatus, VerifyStatus,
};
use chrono::Local;

fn task(id: &str, status: TaskStatus) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None,
        observed_model: None,
        attribution_source: None,
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

fn narration_miss() -> ResultDelivery {
    ResultDelivery::LogFallback {
        looks_like_report: false,
    }
}

#[test]
fn record_missing_report_flags_narration_when_no_prior_cause() {
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-miss-report".to_string());
    store
        .insert_task(&task(task_id.as_str(), TaskStatus::Done))
        .unwrap();

    record_missing_report(&store, &task_id, narration_miss());

    let loaded = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(
        loaded.delivery_assessment,
        Some(DeliveryAssessment::MissingFinalDelivery)
    );
    let latest = store.latest_error(task_id.as_str()).unwrap();
    assert!(latest.contains("Missing final delivery"));
    let events = store.get_events(task_id.as_str()).unwrap();
    assert!(events.iter().any(|e| {
        e.event_kind == EventKind::Error
            && e.metadata.as_ref().is_some_and(|m| {
                m.get("delivery_guard").and_then(|v| v.as_str()) == Some("missing_final_delivery")
            })
    }));
}

#[test]
fn record_missing_report_keeps_prior_kill_cause() {
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-prior-cause".to_string());
    store
        .insert_task(&task(task_id.as_str(), TaskStatus::Failed))
        .unwrap();
    let cause = "Background worker failed: Failed to build agent command: qwen agent does not support read-only mode";
    store
        .insert_event(&TaskEvent {
            task_id: task_id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Error,
            detail: cause.to_string(),
            metadata: None,
        })
        .unwrap();

    record_missing_report(&store, &task_id, narration_miss());

    let loaded = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.delivery_assessment, None);
    assert_eq!(store.latest_error(task_id.as_str()).as_deref(), Some(cause));
    let events = store.get_events(task_id.as_str()).unwrap();
    assert!(!events.iter().any(|e| e.detail.contains("Missing final delivery")));
}
