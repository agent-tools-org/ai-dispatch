// Worker stderr capture across detached stdio redirection.
// Runs redirection in a subprocess to preserve the test runner's descriptors.
use super::*;

#[test]
#[cfg(unix)]
fn worker_panic_survives_stdio_detachment_and_log_reopen() {
    let _permit = crate::support::test_subprocess::acquire();
    let temp = tempfile::tempdir().expect("home");
    let _home = crate::support::paths::AidHomeGuard::set(temp.path());
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
    let log = std::fs::read_to_string(crate::support::paths::task_dir(id).join("worker.log"))
        .expect("read log");
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
