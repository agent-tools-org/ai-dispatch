// E2E coverage for task settlement: exit codes and retries around a failing verify.
// A verifier held on a gate file makes the settle window observable and deterministic.
// Deps: compiled aid binary, tests/common/settlement.rs harness, tempfile, rusqlite.
#![cfg(unix)]

#[allow(dead_code)]
mod common;
#[path = "common/settlement.rs"]
mod settlement;

use settlement::{Harness, TERMINAL};
use std::process::Stdio;
use std::thread;
use std::time::Duration;

const SETTLE: Duration = Duration::from_secs(60);

fn spawn_bg(harness: &Harness, id: &str, extra: &[&str]) {
    let mut args = vec!["--bg"];
    args.extend_from_slice(extra);
    let output = harness.run(id, &args).output().unwrap();
    assert!(
        output.status.success(),
        "--bg dispatch failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn foreground_run_exits_one_when_verify_fails() {
    let harness = Harness::new();
    let child = harness
        .run("t-settle-fg-fail", &[])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    harness.release(1);
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(harness.status("t-settle-fg-fail").as_deref(), Some("failed"));
}

#[test]
fn wait_exit_code_matches_outcome_after_failing_verify() {
    let harness = Harness::new();
    let id = "t-settle-wait-fail";
    spawn_bg(&harness, id, &[]);
    let waiter = harness
        .aid()
        .args(["wait", id, "--timeout", "110"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    harness.release(1);
    let output = waiter.wait_with_output().unwrap();
    assert_eq!(harness.wait_terminal(id), "failed");
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn verify_failure_with_retry_ends_with_child_exit_code() {
    let harness = Harness::new();
    let parent = "t-settle-retry";
    let attach = harness
        .run(parent, &["--retry", "1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    harness.release(1);
    let child = harness.wait_for(SETTLE, |h| h.child_of(parent));
    // The child passes, so a foreground exit of 0 can only come from the child's outcome.
    harness.release(0);
    let output = attach.wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(harness.wait_terminal(&child), "done");
    assert_eq!(harness.status(parent).as_deref(), Some("failed"));
}

#[test]
#[ignore = "task status is terminal before verification settles"]
fn status_is_not_terminal_while_verify_is_gated() {
    let harness = Harness::new();
    let id = "t-settle-gated-status";
    spawn_bg(&harness, id, &[]);
    harness.wait_verifier_holding();
    let status = harness.status(id).unwrap_or_default();
    assert!(!TERMINAL.contains(&status.as_str()), "status {status} is terminal while verify is gated");
    harness.release(0);
    assert_eq!(harness.wait_terminal(id), "done");
}

#[test]
#[ignore = "accept is allowed before verification settles"]
fn accept_is_refused_while_verify_is_gated() {
    let harness = Harness::with_git_project();
    let id = "t-settle-gated-accept";
    spawn_bg(&harness, id, &["-w", "test/settle-accept"]);
    harness.wait_verifier_holding();
    let refused = harness.aid().args(["accept", id]).output().unwrap();
    assert!(
        !refused.status.success(),
        "accept succeeded while verify was gated: stdout={}",
        String::from_utf8_lossy(&refused.stdout)
    );
    harness.release(0);
    assert_eq!(harness.wait_terminal(id), "done");
    let accepted = harness.aid().args(["accept", id]).output().unwrap();
    assert!(
        accepted.status.success(),
        "accept failed after verify passed: stderr={}",
        String::from_utf8_lossy(&accepted.stderr)
    );
}

#[test]
#[ignore = "batch resolves dependencies before parent verification settles"]
fn batch_dependent_waits_for_parent_verify() {
    let harness = Harness::new();
    let (parent, dependent) = ("t-settle-batch-parent", "t-settle-batch-dependent");
    let batch = harness.project().join("batch.toml");
    std::fs::write(
        &batch,
        format!(
            "[defaults]\nagent = \"{}\"\ndir = \"{}\"\nverify = \"{}\"\n\n\
             [[task]]\nid = \"{parent}\"\nname = \"parent\"\nprompt = \"parent\"\n\n\
             [[task]]\nid = \"{dependent}\"\nname = \"dependent\"\nprompt = \"dependent\"\n\
             depends_on = [\"parent\"]\n",
            settlement::AGENT,
            harness.project().display(),
            harness.verifier().display(),
        ),
    )
    .unwrap();
    // Both tasks share the project dir but run one after the other, so --force is safe.
    let mut batch_run = harness
        .aid()
        .arg("batch")
        .arg(&batch)
        .args(["--parallel", "--yes", "--force"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    harness.wait_verifier_holding();
    // Give a dispatcher that ignores the verify barrier time to launch or skip the dependent.
    thread::sleep(Duration::from_secs(3));
    let early = harness.status(dependent);
    assert!(
        matches!(early.as_deref(), None | Some("waiting") | Some("pending")),
        "dependent left its dependency wait before parent verify passed: {early:?}"
    );
    let batch_status = batch_run.try_wait().unwrap();
    assert!(batch_status.is_none(), "batch exited before parent verification settled: {batch_status:?}");
    harness.release(0);
    harness.release(0);
    assert_eq!(harness.wait_terminal(dependent), "done");
    let _ = batch_run.kill();
    let _ = batch_run.wait();
}
