// Cargo verification regression for build-box lock queue accounting.
// Drives the Cargo stream/wait boundary with a fake remote command.
// Deps: build wait, stream pumps, in-memory Store and Tokio process.

use super::*;
use crate::cmd::build::build_process::{ProgressConfig, ProgressState};
use crate::cmd::build::build_stream::pump_lines;
use std::process::Stdio;

#[tokio::test]
async fn cargo_verify_lock_queue_time_does_not_consume_verify_timeout() {
    let _permit = crate::support::test_subprocess::acquire();
    let store = Store::open_memory().expect("store");
    let mut child = tokio::process::Command::new("sh")
        .args([
            "-c",
            concat!(
                "echo 'rbox: waiting for lock (timeout 10 s); holder test' >&2; ",
                "sleep 1; echo 'rbox: job test-job' >&2; exec sleep 0.1",
            ),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("child");
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(pump_lines(
        child.stdout.take().expect("stdout"),
        tx.clone(),
        StreamEvent::Stdout,
    ));
    tokio::spawn(pump_lines(
        child.stderr.take().expect("stderr"),
        tx,
        StreamEvent::Stderr,
    ));
    let mut progress = ProgressState::new(ProgressConfig::for_tests(10000, 10000, 0));
    let mut state = CargoStreamState::default();
    let (status, timed_out) = wait_for_cargo(
        &mut child,
        &mut rx,
        &store,
        &None,
        "cargo check",
        Instant::now(),
        &mut progress,
        &mut state,
        Some(Duration::from_millis(400)),
    )
    .await
    .expect("wait");
    assert!(!timed_out);
    assert!(status.success());
}
