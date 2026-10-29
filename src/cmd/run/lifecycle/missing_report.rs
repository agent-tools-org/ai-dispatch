// Record when a requested result file never became a real report.
// Exports: record_missing_report for the post-run result-file fallback path.
// Deps: Store, DeliveryAssessment, ResultDelivery, TaskEvent, TaskStatus.

use super::super::run_prompt::ResultDelivery;
use crate::store::Store;
use crate::types::{DeliveryAssessment, EventKind, TaskEvent, TaskId, TaskStatus};

/// A requested report that never materialized used to be papered over with whatever the
/// agent had printed, leaving a `done` task whose `result.md` is a tool log. Record the
/// miss so `aid show`, `aid board`, and the JSON view all report it.
///
/// When the task already failed for a terminal kill cause (quota, unsupported flags, spawn
/// failure), skip the Error event so `latest_error` keeps that cause. Still write
/// `delivery_assessment` — that is a delivery fact, not the failure diagnosis.
pub(crate) fn record_missing_report(
    store: &Store,
    task_id: &TaskId,
    delivery: ResultDelivery,
    required: bool,
) {
    if !required || !matches!(delivery, ResultDelivery::MissingFile { .. }) {
        return;
    }
    if let Err(err) = store.update_delivery_assessment(
        task_id.as_str(),
        Some(DeliveryAssessment::MissingFinalDelivery),
    ) {
        aid_warn!("[aid] Failed to record missing delivery: {err}");
    }
    if suppress_missing_report_event(store, task_id) {
        return;
    }
    let event_kind = match crate::task::lifecycle::mark_failed(store, task_id) {
        Ok(true) => EventKind::Error,
        Ok(false) => EventKind::Milestone,
        Err(err) => {
            aid_warn!("[aid] Failed to mark missing required result file: {err}");
            EventKind::Milestone
        }
    };
    let _ = store.insert_event(&TaskEvent::new(
        task_id.clone(),
        event_kind,
        "Missing final delivery: the explicitly required result file was not written"
            .to_string(),
    ).with_metadata(Some(serde_json::json!({
            "delivery_guard": "missing_final_delivery",
            "source": "result_file_fallback",
        }))));
}

/// Suppress the diagnostic Error only when the run already ended Failed with a recorded
/// cause. A Done task that carries an unrelated mid-run error must still be flagged.
fn suppress_missing_report_event(store: &Store, task_id: &TaskId) -> bool {
    let Ok(Some(task)) = store.get_task(task_id.as_str()) else {
        return false;
    };
    task.status == TaskStatus::Failed && store.latest_error(task_id.as_str()).is_some()
}
