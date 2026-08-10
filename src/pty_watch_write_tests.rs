// Regression coverage for queued PTY input delivery after the child exits.
// Exports: monitor_bridge write-failure regression coverage.
// Deps: PtyBridge, monitor_bridge, Store, and persisted task messages.

use std::io::Read;
use std::sync::{Arc, mpsc};

use super::{MonitorState, monitor_bridge};
use crate::agent::codex::CodexAgent;
use crate::pty_bridge::PtyBridge;
use crate::store::Store;
use crate::types::{EventKind, MessageDirection, MessageSource, TaskStatus};

fn spawn_reader(bridge: &mut PtyBridge) -> mpsc::Receiver<Vec<u8>> {
    let mut reader = bridge.take_reader().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 1024];
        while let Ok(size) = reader.read(&mut buffer) {
            if size == 0 || tx.send(buffer[..size].to_vec()).is_err() {
                break;
            }
        }
    });
    rx
}

#[test]
fn queued_message_after_child_exit_does_not_fail_monitor() {
    let _permit = crate::test_subprocess::acquire();
    let temp_home = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp_home.path());
    let task = super::tests::pty_task("t-closed-pty", TaskStatus::Running);
    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&task).unwrap();
    store
        .insert_message(
            task.id.as_str(),
            MessageDirection::In,
            "queued after exit",
            MessageSource::Reply,
        )
        .unwrap();

    let command = vec!["/usr/bin/true".to_string()];
    let mut bridge = PtyBridge::spawn(&command, None, vec![]).unwrap();
    let receiver = spawn_reader(&mut bridge);
    let mut log = tempfile::NamedTempFile::new().unwrap();
    let mut state = MonitorState::new(true, None);

    let result = monitor_bridge(
        &CodexAgent,
        &task.id,
        &store,
        &mut bridge,
        &receiver,
        log.as_file_mut(),
        &mut state,
        None,
        None,
    );

    assert!(
        result.is_ok(),
        "PTY write failure must not fail the monitor: {result:?}"
    );
    assert!(bridge.wait().unwrap().success());

    let message = store.list_messages_for_task(task.id.as_str()).unwrap();
    assert!(message[0].delivered_at.is_none());
    let event = store
        .get_events(task.id.as_str())
        .unwrap()
        .into_iter()
        .find(|event| {
            event
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("input_delivery"))
                .and_then(|value| value.as_str())
                == Some("failed")
        })
        .expect("failed PTY input must be visible as an event");
    assert_eq!(event.event_kind, EventKind::Error);
    assert!(event.detail.contains("Reply not delivered"));
    assert!(
        event.detail.contains("Input/output error") || event.detail.contains("os error 5"),
        "event must preserve the write failure reason: {}",
        event.detail
    );
}
