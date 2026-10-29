// Skip and timeout recording helpers for verification outcomes.
// Exports: nothing_to_verify_reason, record_verify_timed_out.
// Deps: chrono, store, types, verify.

use chrono::Local;

use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent, TaskId};
use crate::task::verify::VerifyResult;

/// Skip verify only when there is genuinely nothing under test.
/// Read-only tasks never change the tree. An empty diff is *not* a skip:
/// "agent delivered nothing" is delivery assessment, and a configured verify
/// must still run against the tree (and can fail if the tree is already broken).
pub(super) fn nothing_to_verify_reason(task: Option<&Task>) -> Option<&'static str> {
    if task.is_some_and(|task| task.read_only) {
        return Some("task is read-only");
    }
    None
}

pub(super) fn record_verify_timed_out(store: &Store, task_id: &TaskId, result: &VerifyResult) {
    let detail = match super::verify_output_excerpt(&result.output) {
        Some(output) => format!(
            "Verification did not finish: {}\nOutput: {output}",
            result.command
        ),
        None => format!("Verification did not finish: {}", result.command),
    };
    let _ = store.insert_event(&TaskEvent::new(task_id.clone(), EventKind::Milestone, detail));
    aid_warn!(
        "[aid] Verification timed out for {task_id}; task not failed (inconclusive)"
    );
}

// Keep result classification shared by foreground and background verification.
pub(super) fn record_result(
    store: &Store,
    task_id: &TaskId,
    result: &VerifyResult,
    cargo_target_dir: Option<&str>,
) {
    use super::{
        record_verify_failed, record_verify_infrastructure_detail, verify_failure_hint,
        verify_infrastructure_detail, verify_output_excerpt,
    };
    let report = crate::task::verify::format_verify_report(result);
    println!("{report}");
    let target_permission_failure = result.infrastructure_failure
        || crate::task::verify::cargo::target_dir_permission_failure(result, cargo_target_dir);
    if target_permission_failure {
        let detail = verify_infrastructure_detail(result);
        record_verify_infrastructure_detail(store, task_id, detail);
    } else {
        crate::task::verify::record_verify_status(store, task_id, result);
    }
    if result.timed_out && !target_permission_failure {
        record_verify_timed_out(store, task_id, result);
    } else if !result.success && !target_permission_failure {
        let hint = verify_failure_hint(store, task_id, &result.output);
        let detail = match verify_output_excerpt(&result.output) {
            Some(output) => {
                format!(
                    "Failed during verification: {}\nOutput: {}{}",
                    result.command,
                    output,
                    hint.as_deref()
                        .map(|value| format!("\n{value}"))
                        .unwrap_or_default()
                )
            }
            None => format!(
                "Failed during verification: {}{}",
                result.command,
                hint.as_deref()
                    .map(|value| format!("\n{value}"))
                    .unwrap_or_default()
            ),
        };
        record_verify_failed(store, task_id, detail);
    }
}
