// Tests for streaming output persistence in `aid run`.
// Covers substantive message retention, filtering, and mixed streaming completion events.
// Depends on super::write_streaming_output, serde_json, and tempfile.

use super::{spawn_child_with_log, write_streaming_output};
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use serde_json::{Value, json};
use tempfile::NamedTempFile;

#[test]
fn write_streaming_output_keeps_last_five_substantive_messages() {
    let log_file = NamedTempFile::new().unwrap();
    let out_file = NamedTempFile::new().unwrap();
    let content = [
        json!({
            "type": "item.completed",
            "item": { "type": "agent_message", "text": "short ack" }
        }),
        json!({
            "type": "item.completed",
            "item": { "type": "agent_message", "text": "Message one is long enough to survive the substantive filter threshold." }
        }),
        json!({
            "type": "message",
            "role": "assistant",
            "content": "Message two starts in a streamed chunk and remains comfortably above ",
            "delta": true
        }),
        json!({
            "type": "message",
            "role": "assistant",
            "content": "the substantive cutoff once the second streamed chunk arrives.",
            "delta": true
        }),
        json!({
            "type": "item.completed",
            "item": { "type": "agent_message", "text": "Message three is another detailed update that should be retained." }
        }),
        json!({
            "type": "message",
            "role": "assistant",
            "content": "Message four is a buffered assistant message with enough detail to keep."
        }),
        json!({
            "type": "item.completed",
            "item": { "type": "agent_message", "text": "Message five records a later milestone with enough context to count." }
        }),
        json!({
            "type": "item.completed",
            "item": { "type": "agent_message", "text": "Message six is the newest substantive message and should push the oldest one out." }
        }),
    ]
    .iter()
    .map(serde_json::to_string)
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
    .join("\n");
    std::fs::write(log_file.path(), content).unwrap();

    write_streaming_output(log_file.path(), out_file.path());

    let output = std::fs::read_to_string(out_file.path()).unwrap();
    assert_eq!(
        output,
        [
            "Message two starts in a streamed chunk and remains comfortably above the substantive cutoff once the second streamed chunk arrives.",
            "Message three is another detailed update that should be retained.",
            "Message four is a buffered assistant message with enough detail to keep.",
            "Message five records a later milestone with enough context to count.",
            "Message six is the newest substantive message and should push the oldest one out.",
        ]
        .join("\n\n---\n\n")
    );
}

#[test]
fn write_streaming_output_skips_writing_when_messages_are_not_substantive() {
    let log_file = NamedTempFile::new().unwrap();
    let out_file = NamedTempFile::new().unwrap();
    let content = [
        json!({
            "type": "item.completed",
            "item": { "type": "agent_message", "text": "short ack" }
        }),
        json!({
            "type": "message",
            "role": "assistant",
            "content": "tiny delta",
            "delta": true
        }),
    ]
    .iter()
    .map(serde_json::to_string)
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
    .join("\n");
    std::fs::write(log_file.path(), content).unwrap();
    std::fs::write(out_file.path(), "existing output").unwrap();

    write_streaming_output(log_file.path(), out_file.path());

    let output = std::fs::read_to_string(out_file.path()).unwrap();
    assert_eq!(output, "existing output");
}

#[test]
fn write_streaming_output_does_not_duplicate_streamed_message_when_final_message_matches() {
    let log_file = NamedTempFile::new().unwrap();
    let out_file = NamedTempFile::new().unwrap();
    let message = "This streamed message is long enough to remain substantive after assembly.";
    let content = [
        json!({
            "type": "message",
            "role": "assistant",
            "content": "This streamed message is long enough ",
            "delta": true
        }),
        json!({
            "type": "message",
            "role": "assistant",
            "content": "to remain substantive after assembly.",
            "delta": true
        }),
        json!({
            "type": "message",
            "role": "assistant",
            "content": message
        }),
    ]
    .iter()
    .map(serde_json::to_string)
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
    .join("\n");
    std::fs::write(log_file.path(), content).unwrap();

    write_streaming_output(log_file.path(), out_file.path());

    let output = std::fs::read_to_string(out_file.path()).unwrap();
    assert_eq!(output, message);
}

#[tokio::test]
async fn spawn_child_with_log_writes_error_event_when_spawn_fails() {
    let log_file = NamedTempFile::new().unwrap();
    let mut cmd = tokio::process::Command::new("/definitely/missing/aid-binary");

    let err = spawn_child_with_log(&mut cmd, log_file.path()).err().unwrap();

    assert!(err.to_string().contains("No such file") || err.to_string().contains("cannot find"));

    let content = std::fs::read_to_string(log_file.path()).unwrap();
    let value: Value = serde_json::from_str(content.trim()).unwrap();
    assert_eq!(value["type"], "error");
    assert_eq!(value["source"], "spawn");
    assert!(
        value["message"]
            .as_str()
            .unwrap()
            .contains("Failed to spawn agent process")
    );
    assert!(value["timestamp"].as_str().is_some());
}

fn task_fixture(id: &str, status: TaskStatus, worktree_path: Option<&str>) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: worktree_path.map(str::to_string),
        worktree_branch: worktree_path.map(|_| "aid-test".to_string()),
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        model: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
    }
}

#[test]
fn record_execution_failure_stores_phase_event_and_snapshot() {
    let aid_home = tempfile::tempdir().unwrap();
    let _guard = crate::paths::AidHomeGuard::set(aid_home.path());
    crate::paths::ensure_dirs().unwrap();
    let store = Store::open_memory().unwrap();
    let task = task_fixture("t-fast-fail", TaskStatus::Running, Some("/tmp/aid-wt-fast-fail"));
    store.insert_task(&task).unwrap();
    std::fs::write(
        crate::paths::stderr_path(task.id.as_str()),
        "spawn blew up\nsecondary detail\n",
    )
    .unwrap();
    let workdir = tempfile::tempdir().unwrap();
    let mut cmd = tokio::process::Command::new("/bin/sh");
    cmd.current_dir(workdir.path());

    let context = super::super::run_prompt::capture_failure_context(&store, &task.id, &cmd);
    super::super::run_prompt::record_execution_failure(&store, &task.id, 1_200, Some(1), &context);

    let events = store.get_events(task.id.as_str()).unwrap();
    assert!(events.iter().any(|event| {
        event.detail.contains("Failed during execution: agent exited with code 1")
            && event.detail.contains("Stderr: spawn blew up | secondary detail")
    }));
    assert!(events.iter().any(|event| {
        event.detail.contains("Failure context: working directory:")
            && event.detail.contains("agent binary: /bin/sh")
            && event.detail.contains("worktree path: /tmp/aid-wt-fast-fail")
            && event.detail.contains("worktree created: true")
    }));
}

#[test]
fn resolve_failure_exit_code_reads_completion_event_detail() {
    let store = Store::open_memory().unwrap();
    let task = task_fixture("t-exit-code", TaskStatus::Failed, None);
    store.insert_task(&task).unwrap();
    store
        .insert_event(&crate::types::TaskEvent {
            task_id: task.id.clone(),
            timestamp: Local::now(),
            event_kind: crate::types::EventKind::Error,
            detail: "FAIL — 0 events, exit code 7".to_string(),
            metadata: None,
        })
        .unwrap();

    let exit_code =
        super::super::run_prompt::resolve_failure_exit_code(&store, &task.id, None);

    assert_eq!(exit_code, Some(7));
}
