// Fake-rbox shell verification regressions for pre-command diagnostics and exit collisions.
// Covers infrastructure status enforcement and conservative no-replay classification.
use super::*;
use crate::support::test_env::write_executable;
use crate::task::verify::{self, VerifyBudget};

fn fake_verify(diagnostic: &str, exit: i32) -> verify::VerifyResult {
    let temp = tempfile::tempdir().expect("temp");
    write_executable(
        &temp.path().join("rbox"),
        &format!("#!/bin/sh\ncat <<'DIAGNOSTIC'\n{diagnostic}\nDIAGNOSTIC\nexit {exit}\n"),
    );
    verify::run_verify_with_env(
        temp.path(),
        Some("rbox exec fake-box"),
        None,
        None,
        VerifyBudget::Duration(std::time::Duration::from_secs(5)),
        &[("PATH".into(), format!("{}:/usr/bin:/bin", temp.path().display()))],
    )
    .expect("fake verify")
}

#[test]
fn confirmed_precommand_failures_are_infrastructure_regardless_of_exit_number() {
    for line in [
        "rbox: tailnet-offline (probe age 0.0s): box: offline (tailnet-offline)",
        "rbox: box: expected stamp abc, actual missing; run rbox ensure box explicitly",
        "rbox: ensure failed: exit code 1; see log",
        "rbox: sync to work/repo (bundle 12 bytes): transfer failed",
        "rbox: Box lock not acquired within 60 s",
        "rbox: box 9.9 GiB free; requires 10 GiB",
    ] {
        for exit in [1, 69, 75, 124] {
            let result = fake_verify(line, exit);
            assert!(result.infrastructure_failure, "{line}: {exit}");
            assert!(!result.success);
            assert!(verify::format_verify_report(&result).contains("INFRASTRUCTURE"));
        }
    }
}

#[test]
fn completed_test_failures_and_numeric_collisions_remain_failed_verify() {
    for exit in [1, 69, 75, 124] {
        for output in [
            "error: compiler rejected source".to_string(),
            format!("rbox: Box lock not acquired within 60 s\nrbox: job abc exited with code {exit}"),
            "rbox: warning: skipping uninitialized submodules: lib/test".to_string(),
        ] {
            let result = fake_verify(&output, exit);
            assert!(!result.infrastructure_failure, "{output}: {exit}");
            assert!(!result.success);
        }
    }
}

#[test]
fn running_or_lost_jobs_are_never_replayed_as_precommand_failures() {
    for state in [
        "rbox: job abc still running after 10 s; the job continues on the box",
        "rbox: warning: job abc is running; recovery record unavailable: offline",
        "rbox: infrastructure: box lost mid-job; receipt unavailable",
        "rbox: job abc",
    ] {
        let output = format!("rbox: tailnet-offline (probe age 0.0s): box: offline\n{state}");
        assert_eq!(diagnostic(&output), None);
        assert!(!retryable(&output));
    }
    assert!(!retryable(
        "rbox: sync to work/repo (bundle 12 bytes): unsupported input"
    ));
    assert_eq!(
        diagnostic("a test printed rbox: Box lock not acquired within 60 s"),
        None
    );
    assert_eq!(
        diagnostic("rbox: box: expected stamp abc, actual old; rbox/bootstrap changed"),
        None
    );
}

#[test]
fn infrastructure_status_preserves_done_but_completed_failure_fails_the_gate() {
    for (output, expected) in [
        (
            "rbox: Box lock not acquired within 60 s",
            crate::types::VerifyStatus::InfrastructureFailure,
        ),
        (
            "rbox: job abc exited with code 75",
            crate::types::VerifyStatus::Failed,
        ),
    ] {
        let store = crate::store::Store::open_memory().expect("store");
        store
            .db()
            .execute(
                "INSERT INTO tasks (id, agent, prompt, status, created_at) VALUES
             ('t-infra', 'codex', 'task', 'done', ?1)",
                [chrono::Local::now().to_rfc3339()],
            )
            .expect("task");
        let id = crate::types::TaskId("t-infra".into());
        let result = fake_verify(output, 75);
        verify::record_verify_status(&store, &id, &result);
        verify::enforce_verify_status(&store, &id);
        let task = store.get_task(id.as_str()).expect("load").expect("task");
        assert_eq!(task.verify_status, expected);
        assert_eq!(
            task.status,
            if result.infrastructure_failure {
                crate::types::TaskStatus::Done
            } else {
                crate::types::TaskStatus::Failed
            }
        );
    }
}
