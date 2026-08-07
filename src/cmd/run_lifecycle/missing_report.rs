// Record when a requested result file never became a real report.
// Exports: record_missing_report for the post-run result-file fallback path.
// Deps: Store, DeliveryAssessment, ResultDelivery, TaskEvent.

use super::super::run_prompt::ResultDelivery;
use crate::store::Store;
use crate::types::{DeliveryAssessment, EventKind, TaskEvent, TaskId};

/// A requested report that never materialized used to be papered over with whatever the
/// agent had printed, leaving a `done` task whose `result.md` is a tool log. Record the
/// miss so `aid show`, `aid board`, and the JSON view all report it.
///
/// When aid already recorded why the run died (quota, unsupported flags, spawn failure),
/// skip this secondary symptom — otherwise `latest_error` becomes "Missing final delivery"
/// and hides the real cause.
pub(crate) fn record_missing_report(
    store: &Store,
    task_id: &TaskId,
    delivery: ResultDelivery,
) {
    if delivery != (ResultDelivery::LogFallback { looks_like_report: false }) {
        return;
    }
    if store.latest_error(task_id.as_str()).is_some() {
        return;
    }
    if let Err(err) = store.update_delivery_assessment(
        task_id.as_str(),
        Some(DeliveryAssessment::MissingFinalDelivery),
    ) {
        aid_warn!("[aid] Failed to record missing delivery: {err}");
    }
    let _ = store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: EventKind::Error,
        detail: "Missing final delivery: no result file written and captured output is tool narration, not a report".to_string(),
        metadata: Some(serde_json::json!({
            "delivery_guard": "missing_final_delivery",
            "source": "result_file_fallback",
        })),
    });
}
