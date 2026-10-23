// stream output tests.
// Uses the shared test fixtures and imports from the parent test module.

use super::*;

#[tokio::test]
async fn streaming_watch_populates_success_exit_code() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-exit-code".to_string());
    insert_running_task(store.as_ref(), &task_id);
    let log_path = temp.path().join("stream.log");
    let mut child = tokio::process::Command::new("sh")
        .current_dir(temp.path())
        .arg("-c")
        .arg("printf 'done\\n'; exit 0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let info = watch_streaming_with_input(
        &StubStreamingAgent,
        &mut child,
        &task_id,
        &store,
        &log_path,
        None,
        crate::config::idle_timeout::DEFAULT_IDLE_TIMEOUT,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    assert_eq!(info.status, TaskStatus::Done);
    assert_eq!(info.exit_code, Some(0));
}
