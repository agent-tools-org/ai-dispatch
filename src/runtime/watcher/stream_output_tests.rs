// stream output tests.
// Uses the shared test fixtures and imports from the parent test module.

use super::*;

#[tokio::test]
async fn streaming_watch_populates_success_exit_code() {
    let (temp, store) = crate::support::test_env::TestHome::new().shared_store();
    let task_id = TaskId("t-exit-code".to_string());
    insert_running_task(store.as_ref(), &task_id);
    let log_path = temp.path().join("stream.log");
    let mut child = crate::support::test_command::piped_shell(temp.path(), "printf 'done\\n'; exit 0");

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

    assert_eq!(info.status, TaskStatus::Done);
    assert_eq!(info.exit_code, Some(0));
}

#[tokio::test]
async fn streaming_read_error_skips_finish_and_preserves_cleanup_order() {
    let temp = crate::support::test_env::TestHome::new();
    let store = Arc::new(Store::open_memory().expect("store"));
    let task_id = TaskId("t-stream-read-error".into());
    insert_running_task(&store, &task_id);
    let marker = paths::task_dir(task_id.as_str()).join("claude-input.json");
    let log_path = temp.path().join("stream.log");
    let mut child = read_error_child(&marker);
    let result = super::super::watch_streaming_with_input(
        &StubStreamingAgent,
        &mut child,
        &task_id,
        &store,
        &log_path,
        None,
        tokio::time::Duration::from_secs(5),
        None,
        Some("prompt"),
        None,
    )
    .await;
    let error = result.expect_err("invalid UTF-8 must propagate");
    assert_eq!(
        error
            .downcast_ref::<std::io::Error>()
            .expect("read error")
            .kind(),
        std::io::ErrorKind::InvalidData,
    );
    assert!(!marker.exists(), "session marker must be removed");
    assert!(
        store
            .get_events(task_id.as_str())
            .expect("events")
            .is_empty()
    );
    assert!(std::fs::read(log_path).expect("log").is_empty());
    let exit = tokio::time::timeout(tokio::time::Duration::from_secs(5), child.wait())
        .await
        .expect("stdin must close without finalization")
        .expect("wait");
    assert!(exit.success(), "marker must be removed before stdin EOF");
}

fn read_error_child(marker: &std::path::Path) -> tokio::process::Child {
    tokio::process::Command::new("sh")
        .args([
            "-c",
            "read -r prompt; printf '\\377\\n'; cat >/dev/null; test ! -e \"$1\"",
            "sh",
        ])
        .arg(marker)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("child")
}
