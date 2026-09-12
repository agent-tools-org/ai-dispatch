// Regression tests for stderr persistence and watcher log visibility.
// Covers blocked file writes and a child retaining the stderr pipe.
// Deps: watcher stderr helpers, Tokio runtime/process, temporary AID_HOME.

use std::future::{Future, poll_fn};
use std::process::Stdio;
use std::task::Poll;

use super::{append_stderr_to_log, drain_stderr_capture, spawn_stderr_capture};
use crate::{paths, types::TaskId};

#[test]
fn stderr_replay_waits_for_pending_log_write() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = paths::AidHomeGuard::set(temp.path());
        paths::ensure_dirs().unwrap();
        let task_id = TaskId("t-stderr-pending-write".to_string());
        std::fs::write(paths::stderr_path(task_id.as_str()), b"saved stderr\n").unwrap();
        let log_path = temp.path().join("stream.log");
        let mut log = tokio::fs::File::create(&log_path).await.unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            let _ = started_tx.send(());
            let _ = release_rx.recv();
        });
        started_rx.await.unwrap();
        let mut replay = std::pin::pin!(append_stderr_to_log(&mut log, &task_id));
        let returned_before_write = poll_fn(|cx| Poll::Ready(replay.as_mut().poll(cx).is_ready())).await;
        release_tx.send(()).unwrap();
        if !returned_before_write {
            replay.await.unwrap();
        }
        blocker.await.unwrap();
        assert!(!returned_before_write, "stderr replay returned while its log write was still queued");
        assert_eq!(std::fs::read_to_string(log_path).unwrap(), "saved stderr\n");
    });
}

#[tokio::test]
async fn stderr_drain_timeout_preserves_output_before_eof() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().unwrap();
    let task_id = TaskId("t-stderr-held-pipe".to_string());
    let mut child = tokio::process::Command::new("sh")
        .args(["-c", "printf 'No saved session found with ID abc' >&2; exec sleep 30"])
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let capture = spawn_stderr_capture(&mut child, &task_id).unwrap();
    let drained = tokio::time::timeout(
        std::time::Duration::from_secs(5), drain_stderr_capture(capture),
    ).await;
    child.kill().await.unwrap();
    assert!(drained.is_ok(), "stderr drain did not time out the retained pipe");
    let stderr = std::fs::read_to_string(paths::stderr_path(task_id.as_str()));
    assert_eq!(stderr.unwrap(), "No saved session found with ID abc");
}

#[test]
fn stderr_drain_waits_for_file_io_after_child_exit() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = paths::AidHomeGuard::set(temp.path());
        paths::ensure_dirs().unwrap();
        let task_id = TaskId("t-stderr-blocked-file".to_string());
        let mut child = tokio::process::Command::new("sh")
            .args(["-c", "printf 'saved stderr' >&2"])
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.wait().await.unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            let _ = started_tx.send(());
            let _ = release_rx.recv();
        });
        started_rx.await.unwrap();
        let capture = spawn_stderr_capture(&mut child, &task_id).unwrap();
        let mut drain = std::pin::pin!(drain_stderr_capture(capture));
        let returned_before_write = tokio::time::timeout(
            super::STDERR_DRAIN_TIMEOUT + std::time::Duration::from_millis(100),
            &mut drain,
        ).await.is_ok();
        release_tx.send(()).unwrap();
        if !returned_before_write {
            drain.await;
        }
        blocker.await.unwrap();
        assert!(!returned_before_write, "drain returned before the captured stderr was persisted");
        assert_eq!(std::fs::read_to_string(paths::stderr_path(task_id.as_str())).unwrap(), "saved stderr");
    });
}


#[tokio::test]
async fn stderr_drain_times_out_continuously_ready_reader() {
    let temp = tempfile::tempdir().unwrap();
    let (child_exited, exited) = tokio::sync::oneshot::channel();
    let stderr_path = temp.path().join("stderr");
    let handle = tokio::spawn(super::capture_stderr(
        ReadyStderr, stderr_path.clone(), exited,
    ));
    let capture = super::StderrCapture { handle, child_exited };
    let drained = tokio::time::timeout(
        std::time::Duration::from_secs(5), drain_stderr_capture(capture),
    ).await;
    assert!(drained.is_ok(), "ready stderr reads starved the drain deadline");
    assert!(std::fs::metadata(stderr_path).unwrap().len() > 0);
}


struct ReadyStderr;

impl tokio::io::AsyncRead for ReadyStderr {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        buf.put_slice(b"x");
        Poll::Ready(Ok(()))
    }
}
