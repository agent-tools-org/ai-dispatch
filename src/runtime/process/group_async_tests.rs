// Async group termination must reap exited leaders during the grace period.
// Covers prompt shutdown and cleanup of TERM-ignoring descendants in separate sessions.
use super::terminate_child_group;
use crate::support::test_subprocess;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};

async fn ready_child(script: &str) -> (tokio::process::Child, String) {
    let mut child = tokio::process::Command::new("python3")
        .args(["-c", script])
        .process_group(0)
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut ready = String::new();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    tokio::time::timeout(Duration::from_secs(2), stdout.read_line(&mut ready))
        .await
        .unwrap()
        .unwrap();
    (child, ready)
}

#[tokio::test]
async fn exited_group_leader_is_reaped_without_waiting_three_second_grace() {
    let _permit = test_subprocess::acquire();
    let (mut child, ready) =
        ready_child("import time; print('ready', flush=True); time.sleep(60)").await;
    assert_eq!(ready.trim(), "ready");
    tokio::time::timeout(
        Duration::from_millis(500),
        terminate_child_group(&mut child),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(child.id().is_none());
    assert!(!child.wait().await.unwrap().success());
}

#[tokio::test]
async fn async_termination_kills_verified_term_ignoring_descendant_in_separate_session() {
    let _permit = test_subprocess::acquire();
    let (mut child, ready) = ready_child(ESCAPED_TOOL).await;
    let descendant: i32 = ready.trim().parse().unwrap();
    tokio::time::timeout(Duration::from_secs(1), terminate_child_group(&mut child))
        .await
        .unwrap()
        .unwrap();
    assert_descendant_stopped(descendant).await;
    assert!(child.id().is_none());
}

const ESCAPED_TOOL: &str = concat!(
    "import subprocess, sys, time\n",
    "tool = subprocess.Popen([sys.executable, '-c', ",
    "'import os, signal, time; signal.signal(signal.SIGTERM, signal.SIG_IGN); ",
    "print(os.getpid(), flush=True); time.sleep(60)'], ",
    "stdout=subprocess.PIPE, start_new_session=True)\n",
    "print(tool.stdout.readline().decode().strip(), flush=True)\n",
    "time.sleep(60)\n",
);

async fn assert_descendant_stopped(descendant: i32) {
    tokio::time::timeout(Duration::from_secs(1), async {
        let mut tick = tokio::time::interval(Duration::from_millis(10));
        while test_subprocess::is_live(descendant) {
            tick.tick().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn cancelled_grace_period_still_kills_verified_escaped_descendants() {
    let _permit = test_subprocess::acquire();
    let script =
        format!("import signal; signal.signal(signal.SIGTERM, signal.SIG_IGN)\n{ESCAPED_TOOL}");
    let (mut child, ready) = ready_child(&script).await;
    let descendant: i32 = ready.trim().parse().unwrap();
    let result = tokio::time::timeout(
        Duration::from_millis(200),
        terminate_child_group(&mut child),
    )
    .await;
    child.kill().await.unwrap();
    child.wait().await.unwrap();
    assert!(result.is_err());
    assert_descendant_stopped(descendant).await;
}
