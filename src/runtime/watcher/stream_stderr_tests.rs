// stream stderr tests.
// Uses the shared test fixtures and imports from the parent test module.

use super::*;

#[tokio::test]
async fn streaming_watch_fast_fail_preserves_stderr_in_log() {
    let (temp, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    let task_id = TaskId("t-stderr-log".to_string());
    insert_running_task(store.as_ref(), &task_id);
    let log_path = temp.path().join("stream.log");
    let mut child = crate::support::test_command::piped_shell(
        temp.path(),
        "echo 'No saved session found with ID abc' >&2; exit 1",
    );

    let info = watch_streaming(
        &StubStreamingAgent,
        &mut child,
        &task_id,
        &store,
        &log_path,
        None,
        crate::config::idle_timeout::DEFAULT_IDLE_TIMEOUT,
        None,
    )
    .await
    .unwrap();

    assert_eq!(info.status, TaskStatus::Failed);
    let log = std::fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("No saved session found with ID abc"));
}

#[tokio::test]
async fn streaming_watch_signal_killed_process_does_not_replay_stderr() {
    let (temp, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    let task_id = TaskId("t-stderr-kill".to_string());
    insert_running_task(store.as_ref(), &task_id);
    let log_path = temp.path().join("stream.log");
    let mut child = crate::support::test_command::piped_shell(
        temp.path(),
        "echo 'stderr that must not be replayed' >&2; exec sleep 30",
    );

    let info = watch_streaming(
        &StubStreamingAgent,
        &mut child,
        &task_id,
        &store,
        &log_path,
        None,
        std::time::Duration::from_millis(100),
        None,
    )
    .await
    .unwrap();

    assert_eq!(info.status, TaskStatus::Failed);
    let log = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        !log.contains("stderr that must not be replayed"),
        "a signal-killed process must not have its stderr replayed into the log, got: {log}"
    );
}

#[tokio::test]
async fn streaming_watch_caps_preserved_stderr() {
    let (temp, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    let task_id = TaskId("t-stderr-cap".to_string());
    insert_running_task(store.as_ref(), &task_id);
    let log_path = temp.path().join("stream.log");
    let mut child = crate::support::test_command::piped_shell(
        temp.path(),
        "yes x | head -c 200000 >&2; exit 1",
    );

    let info = watch_streaming(
        &StubStreamingAgent,
        &mut child,
        &task_id,
        &store,
        &log_path,
        None,
        crate::config::idle_timeout::DEFAULT_IDLE_TIMEOUT,
        None,
    )
    .await
    .unwrap();

    assert_eq!(info.status, TaskStatus::Failed);
    let log = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        log.contains("[stderr truncated]"),
        "oversized stderr must be capped, got {log} bytes"
    );
    assert!(
        log.len() < 200_000,
        "capped stderr must not copy the full stream, got {} bytes",
        log.len()
    );
}
