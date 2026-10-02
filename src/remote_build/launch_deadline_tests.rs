// Production-boundary deadline regressions across the actual verifier mutex.
// Covers no launch after expiry, reduced wait allowance and unchanged local durations.
// Deps: saved remote policies, isolated child test processes and synchronized lock holder.

use super::deadline_tests::{save_policy, timestamps};
use super::shim_tests::executable;
use super::tests::stored_task;
use super::*;
use chrono::Local;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

fn isolated_runner(test: &str) -> bool {
    if std::env::var("AID_LAUNCH_DEADLINE_CHILD").as_deref() == Ok(test) {
        return true;
    }
    let _permit = crate::test_subprocess::acquire();
    let temp = tempfile::tempdir().expect("child home");
    let output = Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            &format!("remote_build::launch_deadline_tests::{test}"),
            "--nocapture",
        ])
        .env("AID_LAUNCH_DEADLINE_CHILD", test)
        .env("AID_HOME", temp.path())
        .output()
        .expect("isolated runner");
    assert!(output.status.success(), "{output:?}");
    false
}

fn hold_verify_lock(shim: Option<PathBuf>, delay: Duration) -> thread::JoinHandle<()> {
    let (ready, held) = mpsc::channel();
    let holder = thread::spawn(move || {
        let _lock = crate::verify::VERIFY_LOCK.lock().expect("verify lock");
        ready.send(()).expect("signal lock held");
        if let Some(shim) = shim {
            // Shim creation proves verify_on passed its early check and began setup.
            let limit = Instant::now() + Duration::from_secs(5);
            while !shim.is_file() {
                assert!(Instant::now() < limit, "runner never prepared its shim");
                thread::sleep(Duration::from_millis(10));
            }
        }
        thread::sleep(delay);
    });
    held.recv_timeout(Duration::from_secs(5))
        .expect("lock acquired before runner begins");
    holder
}

fn assert_timeout(result: &crate::verify::VerifyResult) {
    assert!(result.timed_out, "{result:?}");
    assert!(!result.success);
    assert!(!result.infrastructure_failure);
    assert_eq!(result.exit_code, None);
    assert_eq!(result.command, "./verify");
}

#[test]
fn saved_remote_deadline_expires_while_waiting_for_verify_lock_without_launch() {
    if !isolated_runner(
        "saved_remote_deadline_expires_while_waiting_for_verify_lock_without_launch",
    ) {
        return;
    }
    let (store, id, _) = stored_task();
    let temp = tempfile::tempdir().expect("temp");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    executable(
        &temp.path().join("verify"),
        "#!/bin/bash\necho launched > launched\necho finished\n",
    );
    save_policy(
        &store,
        &id,
        Duration::from_secs(1),
        Duration::from_secs(900),
    );
    let shim = crate::paths::task_dir(id.as_str()).join("home/.aid-build-shims/cargo");
    let holder = hold_verify_lock(Some(shim.clone()), Duration::from_millis(1500));
    let now = Local::now();
    timestamps(&store, &id, now, Some(now));
    let result = verify(
        &store,
        id.as_str(),
        temp.path(),
        Some("./verify"),
        None,
        None,
    )
    .expect("verify");
    holder.join().expect("lock holder");
    assert!(shim.is_file());
    assert_timeout(&result);
    assert!(result.output.contains("no command launched"));
    assert!(!temp.path().join("launched").exists());
}

#[test]
fn saved_remote_deadline_lock_delay_reduces_command_wait_allowance() {
    if !isolated_runner("saved_remote_deadline_lock_delay_reduces_command_wait_allowance") {
        return;
    }
    let (store, id, _) = stored_task();
    let temp = tempfile::tempdir().expect("temp");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    executable(
        &temp.path().join("verify"),
        "#!/bin/bash\necho launched > launched\necho \"$AID_BUILD_BOX\"\nexec /bin/sleep 2\n",
    );
    save_policy(
        &store,
        &id,
        Duration::from_secs(3),
        Duration::from_secs(900),
    );
    let shim = crate::paths::task_dir(id.as_str()).join("home/.aid-build-shims/cargo");
    let holder = hold_verify_lock(Some(shim), Duration::from_secs(2));
    let now = Local::now();
    timestamps(&store, &id, now, Some(now));
    let result = verify(
        &store,
        id.as_str(),
        temp.path(),
        Some("./verify"),
        None,
        None,
    )
    .expect("verify");
    holder.join().expect("lock holder");
    // A reset three-second allowance would let the two-second command succeed.
    assert!(temp.path().join("launched").is_file());
    assert_timeout(&result);
    assert!(result.output.contains("chosen-box"));
    assert!(result.output.contains("Verification timed out"));
}

#[test]
fn local_duration_starts_after_verify_lock_delay() {
    if !isolated_runner("local_duration_starts_after_verify_lock_delay") {
        return;
    }
    let temp = tempfile::tempdir().expect("temp");
    executable(
        &temp.path().join("verify"),
        "#!/bin/bash\necho launched > launched\n/bin/sleep 0.2\necho local\n",
    );
    let holder = hold_verify_lock(None, Duration::from_millis(800));
    let result = crate::verify::run_verify_with_timeout(
        temp.path(),
        Some("./verify"),
        None,
        None,
        Duration::from_millis(500),
    )
    .expect("local verify");
    holder.join().expect("lock holder");
    assert!(result.success, "{result:?}");
    assert!(!result.timed_out);
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.output, "local\n");
    assert!(temp.path().join("launched").is_file());
}

#[test]
fn expired_env_budget_preserves_explicit_skip_and_no_project_semantics() {
    let temp = tempfile::tempdir().expect("temp");
    let budget = crate::verify::VerifyBudget::Deadline(Local::now() - chrono::Duration::seconds(1));
    for (command, success) in [(Some("skip"), false), (None, true)] {
        let result =
            crate::verify::run_verify_with_env(temp.path(), command, None, None, budget, &[])
                .expect("skipped verification");
        assert_eq!(result.success, success);
        assert!(!result.timed_out);
        assert!(!result.infrastructure_failure);
        assert_eq!(result.command, "skip");
        assert_eq!(result.exit_code, None);
    }
}
