// Verification infrastructure regression tests at the task outcome boundary.
// Exports: no production API; tests exact target denial and genuine Cargo failure.
// Deps: run::maybe_verify, Store, task outcome types, tempfile.

use super::maybe_verify;
use crate::types::outcome::UnverifiedReason;
use crate::{store::Store, types::{Task, TaskId, TaskOutcome, TaskStatus, VerifyStatus}};
use chrono::Local;

fn task(id: &str, dir: &str, verify: &str) -> Task {
    Task { worktree_path: Some(dir.to_string()),
        worktree_branch: Some("fix/verify-gate".to_string()), duration_ms: Some(10), exit_code: Some(0),
        created_at: Local::now(), completed_at: Some(Local::now()), verify: Some(verify.to_string()),
        ..crate::types::Task::test_fixture(TaskId(id.to_string()), TaskStatus::Done)
    }
}

#[cfg(unix)]
#[test]
fn cargo_target_permission_diagnostic_is_unverified_infrastructure() {
    let _permit = crate::support::test_subprocess::acquire();
    let dir = tempfile::tempdir().unwrap();
    let dir_str = dir.path().to_string_lossy().to_string();
    let target = crate::agent::target_dir_for_worktree(Some("fix/verify-gate")).unwrap();
    let script = dir.path().join("target-denied.sh");
    std::fs::write(&script, format!(
        "#!/bin/sh\necho 'error: error writing dependencies to `{target}/debug/deps/foo.d`: Operation not permitted (os error 1)' >&2\nexit 1\n"
    )).unwrap();
    let command = format!("sh {}", script.display());
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-target-denied".to_string());
    store.insert_task(&task(task_id.as_str(), &dir_str, &command)).unwrap();

    maybe_verify(&store, &task_id, Some(&command), Some(&dir_str), None);

    let loaded = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.verify_status, VerifyStatus::InfrastructureFailure);
    assert_eq!(loaded.status, TaskStatus::Done);
    assert_eq!(loaded.outcome(), TaskOutcome::Unverified(UnverifiedReason::Infrastructure));
    let events = store.get_events(task_id.as_str()).unwrap();
    assert!(events.iter().any(|event| {
        event.detail.contains("Operation not permitted") && event.detail.contains(&target)
    }));
}
