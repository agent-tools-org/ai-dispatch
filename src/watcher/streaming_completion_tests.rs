// Streaming completion-status integration tests.
// Replays real ~/.aid/logs fixtures through watch_streaming with exit 0.
// Covers both directions: error envelope → Failed, success log → Done.

use std::process::Stdio;
use std::sync::Arc;

use crate::agent::claude::ClaudeAgent;
use crate::agent::cursor::CursorAgent;
use crate::agent::opencode::OpenCodeAgent;
use crate::agent::qwen::QwenAgent;
use crate::agent::Agent;
use crate::paths;
use crate::store::Store;
use crate::types::TaskStatus;

use super::streaming_tests::insert_running_task;
use super::watch_streaming;

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/streaming_completion/{}",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

async fn watch_exit0(agent: &dyn Agent, output: &str, task_id: &str) -> TaskStatus {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = crate::types::TaskId(task_id.to_string());
    insert_running_task(store.as_ref(), &task_id);
    let log_path = temp.path().join("stream.log");
    let mut child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg("cat; exit 0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        use tokio::io::AsyncWriteExt;
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(output.as_bytes()).await.unwrap();
    }
    let info = watch_streaming(
        agent,
        &mut child,
        &task_id,
        &store,
        &log_path,
        None,
        crate::idle_timeout::DEFAULT_IDLE_TIMEOUT,
        None,
    )
    .await
    .unwrap();
    assert_eq!(info.exit_code, Some(0));
    info.status
}

#[tokio::test]
async fn exit0_qwen_api_error_fixture_records_failed() {
    let output = fixture("qwen-exit0-api-error.jsonl");
    assert!(
        output.contains("[API Error:"),
        "fixture must be real qwen API error log"
    );
    let status = watch_exit0(&QwenAgent, &output, "t-stream-qwen-err").await;
    assert_eq!(status, TaskStatus::Failed);
}

#[tokio::test]
async fn exit0_result_is_error_true_fixture_records_failed() {
    let output = fixture("stream-result-is-error-true.jsonl");
    assert!(
        output.contains("\"is_error\":true"),
        "fixture must carry real is_error:true result"
    );
    let status = watch_exit0(&CursorAgent, &output, "t-stream-cursor-err").await;
    assert_eq!(status, TaskStatus::Failed);
    let status = watch_exit0(&ClaudeAgent, &output, "t-stream-claude-err").await;
    assert_eq!(status, TaskStatus::Failed);
}

#[tokio::test]
async fn exit0_cursor_success_fixture_records_done() {
    let output = fixture("cursor-exit0-success.jsonl");
    assert!(
        output.contains("\"is_error\":false"),
        "fixture must be real cursor success result"
    );
    let status = watch_exit0(&CursorAgent, &output, "t-stream-cursor-ok").await;
    assert_eq!(status, TaskStatus::Done);
}

#[tokio::test]
async fn exit0_opencode_error_envelope_records_failed() {
    let output = fixture("opencode-error-envelope.jsonl");
    assert!(
        output.contains("\"type\":\"error\""),
        "fixture must carry real opencode nested error"
    );
    let status = watch_exit0(&OpenCodeAgent, &output, "t-stream-oc-err").await;
    assert_eq!(status, TaskStatus::Failed);
}

#[tokio::test]
async fn exit0_opencode_success_fixture_records_done() {
    let output = fixture("opencode-exit0-success.jsonl");
    assert!(
        !output.contains("\"type\":\"error\""),
        "success fixture must not include error events"
    );
    let status = watch_exit0(&OpenCodeAgent, &output, "t-stream-oc-ok").await;
    assert_eq!(status, TaskStatus::Done);
}
