// Verification regressions for remote lock queue time versus command execution time.
// Keeps task deadlines absolute while excluding lock waits from duration budgets.
// Deps: verify runner, temporary executable and subprocess permit.

use super::*;
use chrono::Local;

fn lock_wait_command(dir: &Path) {
    let script = dir.join("verify");
    crate::support::test_env::write_executable(
        &script,
        concat!(
            "#!/bin/sh\n",
            "echo 'rbox: waiting for lock (timeout 10 s); holder test' >&2\n",
            "sleep 1\n",
            "echo 'rbox: job test-job' >&2\n",
            "sleep 0.1\n",
            "echo verified\n",
        ),
    );
}

#[test]
fn verify_lock_queue_time_does_not_consume_verify_timeout() {
    let _permit = crate::support::test_subprocess::acquire();
    let dir = tempfile::tempdir().expect("temp");
    lock_wait_command(dir.path());
    let result = run_verify_with_timeout(
        dir.path(),
        Some("./verify"),
        None,
        None,
        Duration::from_millis(400),
    )
    .expect("verify");
    assert!(result.success, "{result:?}");
    assert!(!result.timed_out);
    assert!(result.output.contains("verified"));
}

#[test]
fn verify_lock_queue_still_obeys_task_deadline() {
    let _permit = crate::support::test_subprocess::acquire();
    let dir = tempfile::tempdir().expect("temp");
    lock_wait_command(dir.path());
    let result = run_verify_with_env(
        dir.path(),
        Some("./verify"),
        None,
        None,
        VerifyBudget::Deadline(Local::now() + chrono::Duration::milliseconds(400)),
        &[],
    )
    .expect("verify");
    assert!(result.timed_out);
    assert!(!result.output.contains("verified"));
}
