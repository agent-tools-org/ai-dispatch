// Pins failure attribution for the result-file missing-report guard.
// Covers: suppress event on Failed+kill cause; still flag Done with mid-run error.
// Deps: missing_report, Store, ResultDelivery, task fixtures.

use super::run_lifecycle::record_missing_report;
use super::run_prompt::ResultDelivery;
use crate::store::Store;
use crate::types::{DeliveryAssessment, EventKind, Task, TaskEvent, TaskId, TaskStatus};
use chrono::Local;

fn task(id: &str, status: TaskStatus) -> Task {
    crate::types::Task::test_fixture(TaskId(id.to_string()), status)
}

fn narration_miss() -> ResultDelivery {
    ResultDelivery::MissingFile {
        fallback_saved: true,
    }
}

fn insert_error(store: &Store, task_id: &TaskId, detail: &str) {
    store
        .insert_event(&TaskEvent::new(task_id.clone(), EventKind::Error, detail.to_string()))
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
