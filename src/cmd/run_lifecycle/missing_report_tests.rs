// Pins failure attribution for the result-file missing-report guard.
// Covers: suppress event on Failed+kill cause; still flag Done with mid-run error.
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
        repo_path: None, project_id: None,
        worktree_path: None, effective_dir: None,
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
    ResultDelivery::MissingFile {
        fallback_saved: true,
    }
}

fn insert_error(store: &Store, task_id: &TaskId, detail: &str) {
    store
        .insert_event(&TaskEvent {
            task_id: task_id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Error,
            detail: detail.to_string(),
            metadata: None,
        })
        .unwrap();
}

fn has_missing_delivery_event(store: &Store, task_id: &str) -> bool {
    store.get_events(task_id).unwrap().iter().any(|e| {
        e.event_kind == EventKind::Error
            && e.metadata.as_ref().is_some_and(|m| {
                m.get("delivery_guard").and_then(|v| v.as_str()) == Some("missing_final_delivery")
            })
    })
}

#[test]
fn record_missing_report_flags_narration_when_no_prior_cause() {
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-miss-report".to_string());
    store
        .insert_task(&task(task_id.as_str(), TaskStatus::Done))
        .unwrap();

    record_missing_report(&store, &task_id, narration_miss(), true);

    let loaded = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(
        loaded.delivery_assessment,
        Some(DeliveryAssessment::MissingFinalDelivery)
    );
    assert_eq!(loaded.status, TaskStatus::Failed);
    let latest = store.latest_error(task_id.as_str()).unwrap();
    assert!(latest.contains("Missing final delivery"));
    assert!(has_missing_delivery_event(&store, task_id.as_str()));
}

#[test]
fn record_missing_report_keeps_prior_kill_cause() {
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-prior-cause".to_string());
    store
        .insert_task(&task(task_id.as_str(), TaskStatus::Failed))
        .unwrap();
    let cause = "Background worker failed: Failed to build agent command: qwen agent does not support read-only mode";
    insert_error(&store, &task_id, cause);

    record_missing_report(&store, &task_id, narration_miss(), true);

    // Assessment records the delivery fact; the Error event is withheld so latest_error
    // stays the terminal kill cause.
    let loaded = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(
        loaded.delivery_assessment,
        Some(DeliveryAssessment::MissingFinalDelivery)
    );
    assert_eq!(store.latest_error(task_id.as_str()).as_deref(), Some(cause));
    assert!(!has_missing_delivery_event(&store, task_id.as_str()));
}

#[test]
fn record_missing_report_flags_done_despite_mid_run_error() {
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-done-mid-error".to_string());
    store
        .insert_task(&task(task_id.as_str(), TaskStatus::Done))
        .unwrap();
    insert_error(
        &store,
        &task_id,
        "Error: Exit code 143 Command timed out after 2m 0s",
    );

    record_missing_report(&store, &task_id, narration_miss(), true);

    let loaded = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(
        loaded.delivery_assessment,
        Some(DeliveryAssessment::MissingFinalDelivery)
    );
    assert_eq!(loaded.status, TaskStatus::Failed);
    let latest = store.latest_error(task_id.as_str()).unwrap();
    assert!(latest.contains("Missing final delivery"));
    assert!(has_missing_delivery_event(&store, task_id.as_str()));
}

#[test]
fn auto_result_file_is_advisory() {
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-auto-report".to_string());
    store
        .insert_task(&task(task_id.as_str(), TaskStatus::Done))
        .unwrap();

    record_missing_report(&store, &task_id, narration_miss(), false);

    let loaded = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Done);
    assert_eq!(loaded.delivery_assessment, None);
}
