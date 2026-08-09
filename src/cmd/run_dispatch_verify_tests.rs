// Tests dispatch-time verification state initialization.
// Exports: none; loaded by run_dispatch_prepare.rs under `#[cfg(test)]`.
// Deps: dispatch preparation, Store, and task outcome derivation.

use super::*;
use crate::types::{verify_required, TaskOutcome};
use std::sync::Arc;

#[test]
fn configured_verification_is_pending_when_dispatch_inserts_task() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = crate::paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Run the configured verification command.".to_string(),
        verify: Some("cargo test".to_string()),
        ..Default::default()
    };

    let prepared = prepare_dispatch(&store, &mut args).unwrap();
    let task = store.get_task(prepared.task_id.as_str()).unwrap().unwrap();

    assert_eq!(task.verify_status, VerifyStatus::Pending);
    assert_eq!(prepared.task.verify_status, VerifyStatus::Pending);
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(
        TaskOutcome::derive(task.status, task.verify_status, verify_required(task.verify.as_deref())),
        TaskOutcome::InProgress
    );
}

#[test]
fn failed_task_with_pending_verification_is_still_failed() {
    assert_eq!(
        TaskOutcome::derive(TaskStatus::Failed, VerifyStatus::Pending, true),
        TaskOutcome::Failed
    );
}
