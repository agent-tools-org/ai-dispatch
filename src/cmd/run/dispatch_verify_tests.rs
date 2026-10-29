// Tests dispatch-time verification state initialization.
// Exports: none; loaded by run_dispatch_prepare.rs under `#[cfg(test)]`.
// Deps: dispatch preparation, Store, and task outcome derivation.

use super::*;
use crate::types::{verify_required, TaskOutcome};
use std::sync::Arc;

#[test]
fn configured_verification_is_pending_when_dispatch_inserts_task() {
    let (_temp, store) = crate::support::test_env::TestHome::new().shared_store();
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Run the configured verification command.".to_string(),
        verify: Some("cargo test".to_string()),
        ..Default::default()
    };

    let prepared = prepare_dispatch_with(&store, &mut args, |_| true).unwrap();
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

#[test]
fn auto_verify_prefers_project_command_and_preserves_explicit_commands() {
    let project = crate::config::project::ProjectConfig {
        verify: Some("scripts/remote-test.sh -- --bin aid".into()),
        ..Default::default()
    };
    for (requested, expected) in [
        (None, "scripts/remote-test.sh -- --bin aid"),
        (Some("auto"), "scripts/remote-test.sh -- --bin aid"),
        (Some("custom-check"), "custom-check"),
    ] {
        let mut args = RunArgs {
            verify: requested.map(str::to_string),
            ..Default::default()
        };
        super::super::run_dispatch_resolve::apply_project_defaults(&mut args, Some(&project));
        assert_eq!(args.verify.as_deref(), Some(expected));
    }
    let mut args = RunArgs {
        verify: Some("auto".into()),
        ..Default::default()
    };
    super::super::run_dispatch_resolve::apply_project_defaults(&mut args, None);
    assert_eq!(args.verify.as_deref(), Some("auto"));
}
