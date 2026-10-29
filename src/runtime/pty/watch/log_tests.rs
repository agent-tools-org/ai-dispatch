// Tests for PTY monitor log sanitization.
// Covers raw chunk writes so PTY logs match stdout escape stripping.
// Depends on MonitorState, Store, and tempfile-backed log files.

use super::MonitorState;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus};

use std::sync::Arc;

#[test]
fn handle_chunk_strips_terminal_escapes_before_log_write() {
    let store = Arc::new(Store::open_memory().unwrap());
    let task = pty_task("t-pty-clean-log");
    store.insert_task(&task).unwrap();
    let mut state = MonitorState::new(true, None);
    let mut log = tempfile::NamedTempFile::new().unwrap();

    state
        .handle_chunk(
            &crate::agent::codex::CodexAgent,
            &task.id,
            &store,
            log.as_file_mut(),
            "\x1b]0;title\x07\x1b[32mclean line\x1b[0m\n".to_string(),
        )
        .unwrap();

    let bytes = std::fs::read(log.path()).unwrap();
    assert!(!bytes.contains(&0x1b));
    assert_eq!(String::from_utf8(bytes).unwrap(), "clean line\n");
}

#[test]
fn output_after_pty_write_never_acknowledges_input() {
    use crate::types::{MessageDirection, MessageSource};
    let _permit = crate::support::test_subprocess::acquire();
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = pty_task("t-unconfirmed-input");
    task.agent = AgentKind::Cursor;
    store.insert_task(&task).unwrap();
    store.insert_message(task.id.as_str(), MessageDirection::In, "pivot",
        MessageSource::Reply).unwrap();
    let command = ["/bin/sh", "-c", "read line"].map(String::from).to_vec();
    let mut bridge = crate::runtime::pty::bridge::PtyBridge::spawn(&command, None, vec![]).unwrap();
    let mut state = MonitorState::new(false, None);
    state.maybe_consume_reply(&mut bridge, &store, &task.id).unwrap();
    let mut log = tempfile::NamedTempFile::new().unwrap();
    state.handle_chunk(&crate::agent::cursor::CursorAgent, &task.id, &store,
        log.as_file_mut(), "pivot\nunrelated output\n".to_string()).unwrap();
    state.line_buffer.push_str("trailing output");
    state.flush_trailing_output(&crate::agent::cursor::CursorAgent, &task.id, &store).unwrap();
    assert!(bridge.wait().unwrap().success());
    let messages = store.list_messages_for_task(task.id.as_str()).unwrap();
    assert!(messages[0].delivered_at.is_some());
    assert!(serde_json::to_value(&messages[0]).unwrap().get("acked_at").is_none());
    let events = store.get_events(task.id.as_str()).unwrap();
    assert!(!events.iter().any(|event| event.detail.contains("Acked reply")));
    let metadata = events[0].metadata.as_ref().unwrap();
    assert_eq!(metadata["input_delivery"], "written");
    assert_eq!(metadata["consumption"], "unconfirmed");
    assert!(metadata.get("delivered").is_none());
}

fn pty_task(task_id: &str) -> Task {
    crate::types::Task::test_fixture(TaskId(task_id.to_string()), TaskStatus::Running)
}
