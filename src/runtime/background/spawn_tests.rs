// Worker stderr capture across detached stdio redirection.
// Runs redirection in a subprocess to preserve the test runner's descriptors.
use super::*;

#[test]
#[cfg(unix)]
fn worker_panic_survives_stdio_detachment_and_log_reopen() {
    let _permit = crate::support::test_subprocess::acquire();
    let _temp = crate::support::test_env::TestHome::new();
    let id = "t-worker-panic";
    use std::io::Write;
    writeln!(worker_log(id).expect("log"), "earlier diagnostic").expect("write");
    let status = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            "runtime::background::spawn::tests::detached_panic_helper",
            "--ignored",
            "--nocapture",
        ])
        .stderr(worker_log(id).expect("reopen log"))
        .stdout(std::process::Stdio::null())
        .status()
        .expect("helper");
    assert!(!status.success());
    let log =
        std::fs::read_to_string(crate::support::paths::task_dir(id).join("worker.log")).expect("read log");
    assert!(log.starts_with("earlier diagnostic\n"));
    assert!(log.contains("detached worker panic"));
}

#[test]
#[ignore]
#[cfg(unix)]
fn detached_panic_helper() {
    redirect_worker_stdio().expect("redirect");
    panic!("detached worker panic");
}

#[test]
#[cfg(unix)]
fn pid_handoff_finishes_before_launcher_stdout_closes() {
    use std::io::Write;
    let _permit = crate::support::test_subprocess::acquire();
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "printf '%s\\n' \"$$\"; exec cat"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let (mut launcher, pid) = spawn_prepared(&mut cmd, true).expect("PID line without EOF");
    assert_eq!(pid, launcher.id());
    assert!(launcher.try_wait().expect("status").is_none());
    launcher
        .stdin
        .take()
        .expect("ack pipe")
        .write_all(b"1")
        .expect("ack");
    assert!(launcher.wait().expect("reap").success());
}

#[test]
#[cfg(unix)]
fn stalled_pid_handoff_has_a_deadline() {
    let _permit = crate::support::test_subprocess::acquire();
    let mut launcher = Command::new("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("launcher");
    let start = std::time::Instant::now();
    let result = receive_worker_pid(&mut launcher);
    let elapsed = start.elapsed();
    launcher.kill().expect("kill launcher");
    launcher.wait().expect("reap launcher");
    assert!(
        result
            .expect_err("no PID reported")
            .to_string()
            .contains("timed out")
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "PID wait exceeded its deadline: {elapsed:?}"
    );
}

#[test]
fn pid_handoff_rejects_incomplete_and_unsafe_pids() {
    for line in ["42", "0\n", "1\n", "4294967295\n", "invalid\n"] {
        assert!(parse_worker_pid(line).is_err(), "accepted {line:?}");
    }
    assert_eq!(parse_worker_pid("42\n").expect("PID"), 42);
}

#[test]
fn prepared_spawn_failure_is_reported() {
    let mut cmd = Command::new("/nonexistent/aid-launch-test");
    assert!(spawn_prepared(&mut cmd, false).is_err());
}

#[test]
#[cfg(unix)]
fn worker_requires_launch_acknowledgement() {
    use std::io::Write;
    let _permit = crate::support::test_subprocess::acquire();
    for acknowledged in [false, true] {
        let mut helper = Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "runtime::background::spawn::tests::acknowledgement_helper",
                "--ignored",
                "--nocapture",
            ])
            .env("AID_TEST_ACK", if acknowledged { "1" } else { "0" })
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .expect("helper");
        let mut pipe = helper.stdin.take().expect("pipe");
        if acknowledged {
            pipe.write_all(b"1").expect("ack");
        }
        drop(pipe);
        assert!(helper.wait().expect("reap helper").success());
    }
}

#[test]
#[ignore]
#[cfg(unix)]
fn acknowledgement_helper() {
    assert_eq!(
        worker_acknowledged(),
        std::env::var("AID_TEST_ACK").expect("expectation") == "1"
    );
}
