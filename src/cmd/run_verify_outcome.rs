// Skip and timeout recording helpers for verification outcomes.
// Exports: nothing_to_verify_reason, record_verify_timed_out.
// Deps: chrono, store, types, verify.

use chrono::Local;

use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent, TaskId};
use crate::verify::VerifyResult;

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
    let detail = match output_excerpt(&result.output) {
        Some(output) => format!(
            "Verification did not finish: {}\nOutput: {output}",
            result.command
        ),
        None => format!("Verification did not finish: {}", result.command),
    };
    let _ = store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Milestone,
        detail,
        metadata: None,
    });
    aid_warn!(
        "[aid] Verification timed out for {task_id}; task not failed (inconclusive)"
    );
}

fn output_excerpt(output: &str) -> Option<String> {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }
    let start = lines.len().saturating_sub(8);
    let excerpt = lines[start..].join(" | ");
    Some(if excerpt.chars().count() > 400 {
        let mut truncated: String = excerpt.chars().take(400).collect();
        truncated.push_str("...");
        truncated
    } else {
        excerpt
    })
}
