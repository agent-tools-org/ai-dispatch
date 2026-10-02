// Regression coverage for queued PTY input delivery after the child exits or stops reading.
// Exports: monitor_bridge write-failure regression coverage.
// Deps: PtyBridge, monitor_bridge, Store, and persisted task messages.

use std::io::Read;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use super::{MonitorState, monitor_bridge, write_output_file};
use crate::agent::antigravity::AntigravityAgent;
use crate::agent::codex::CodexAgent;
use crate::input_signal;
use crate::pty_bridge::PtyBridge;
use crate::store::Store;
use crate::types::{AgentKind, EventKind, MessageDirection, MessageSource, TaskStatus};

fn spawn_reader(bridge: &mut PtyBridge) -> mpsc::Receiver<Vec<u8>> {
    spawn_reader_with_error_delay(bridge, Duration::ZERO)
}

fn spawn_reader_with_error_delay(
    bridge: &mut PtyBridge,
    error_delay: Duration,
) -> mpsc::Receiver<Vec<u8>> {
    let mut reader = bridge.take_reader().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 1024];
        loop {
            match reader.read(&mut buffer) {
                Ok(size) if size > 0 => {
                    if tx.send(buffer[..size].to_vec()).is_err() {
                        break;
                    }
                }
                Ok(_) | Err(_) => {
                    std::thread::sleep(error_delay);
                    break;
                }
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
        event.detail.contains("PTY child has already exited")
            || event.detail.contains("Input/output error")
            || event.detail.contains("os error 5"),
        "event must preserve the write failure reason: {}",
        event.detail
    );
}

#[test]
fn failed_pending_reply_is_reported_once_after_successful_steer() {
    let _permit = crate::test_subprocess::acquire();
    let temp_home = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp_home.path());
    let task = super::tests::pty_task("t-write-event-once", TaskStatus::Running);
    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&task).unwrap();
    store
        .insert_message(
            task.id.as_str(),
            MessageDirection::In,
            "first steer",
            MessageSource::Steer,
        )
        .unwrap();
    input_signal::write_steer(task.id.as_str(), "first steer").unwrap();

    let command = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "read line; exit 0".to_string(),
    ];
    let mut bridge = PtyBridge::spawn(&command, None, vec![]).unwrap();
    let receiver = spawn_reader_with_error_delay(&mut bridge, Duration::from_millis(1_500));
    let delayed_store = Arc::clone(&store);
    let delayed_task_id = task.id.clone();
    let delayed_insert = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(750));
        delayed_store
            .insert_message(
                delayed_task_id.as_str(),
                MessageDirection::In,
                "reply after exit",
                MessageSource::Reply,
            )
            .unwrap();
    });
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
        None,
    );

    delayed_insert.join().unwrap();
    assert!(result.is_ok(), "monitor must survive PTY write failure: {result:?}");
    assert!(bridge.wait().unwrap().success());
    let messages = store.list_messages_for_task(task.id.as_str()).unwrap();
    assert!(messages[0].delivered_at.is_some(), "initial steer must succeed");
    assert!(messages[1].delivered_at.is_none(), "failed reply must remain recoverable");
    let failed_events = store
        .get_events(task.id.as_str())
        .unwrap()
        .into_iter()
        .filter(|event| {
            event
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("input_delivery"))
                .and_then(|value| value.as_str())
                == Some("failed")
        })
        .collect::<Vec<_>>();
    assert_eq!(failed_events.len(), 1, "failed reply must produce one event");
    assert_eq!(failed_events[0].event_kind, EventKind::Error);
}

#[test]
fn noninteractive_agent_leaves_queued_input_untouched() {
    let _permit = crate::test_subprocess::acquire();
    let temp_home = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp_home.path());
    let task = super::tests::pty_task("t-noninteractive-input", TaskStatus::Running);
    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&task).unwrap();
    store
        .insert_message(
            task.id.as_str(),
            MessageDirection::In,
            "queued input",
            MessageSource::Reply,
        )
        .unwrap();
    input_signal::write_steer(task.id.as_str(), "queued steer").unwrap();

    let command = vec!["/usr/bin/true".to_string()];
    let mut bridge = PtyBridge::spawn(&command, None, vec![]).unwrap();
    let receiver = spawn_reader(&mut bridge);
    let mut log = tempfile::NamedTempFile::new().unwrap();
    let mut state = MonitorState::new(false, None);

    monitor_bridge(
        &AntigravityAgent,
        &task.id,
        &store,
        &mut bridge,
        &receiver,
        log.as_file_mut(),
        &mut state,
        None,
        None,
        None,
    )
    .unwrap();
    assert!(bridge.wait().unwrap().success());

    let messages = store.list_messages_for_task(task.id.as_str()).unwrap();
    assert!(messages[0].delivered_at.is_none());
    assert_eq!(input_signal::take_steer(task.id.as_str()).unwrap().as_deref(), Some("queued steer"));
}

/// t-73618fc7 / t-dcbd359e: a child that never reads its tty blocked the second
/// steer's PTY write and the monitor stopped reading output for 20 minutes. Claude
/// no longer takes PTY input, so an input-accepting adapter stands in for it.
#[test]
fn input_to_child_that_never_reads_does_not_wedge_monitor() {
    let _permit = crate::test_subprocess::acquire();
    let temp_home = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp_home.path());
    let task = super::tests::pty_task("t-unread-tty", TaskStatus::Running);
    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&task).unwrap();
    let second = format!("{}\n", "b".repeat(99)).repeat(1_400);
    for content in ["a".repeat(852), second] {
        store.insert_message(task.id.as_str(), MessageDirection::In, &content, MessageSource::Steer).unwrap();
    }
    let script = "sleep 4; echo after-input-marker; exec sleep 60";
    let command = ["/bin/sh", "-c", script].map(String::from).to_vec();
    let mut bridge = PtyBridge::spawn(&command, None, vec![]).unwrap();
    let child_pid = bridge.child_pid().unwrap() as i32;
    let receiver = spawn_reader(&mut bridge);
    let log = tempfile::NamedTempFile::new().unwrap();
    let mut log_file = log.reopen().unwrap();
    let (monitor_store, task_id) = (Arc::clone(&store), task.id.clone());
    let (done_tx, done_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut state = MonitorState::new(true, None);
        let result = monitor_bridge(&CodexAgent, &task_id, &monitor_store, &mut bridge,
            &receiver, &mut log_file, &mut state, None, None, None);
        let _ = bridge.wait();
        let _ = done_tx.send(result.is_ok());
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let mut kept_reading = false;
    while !kept_reading && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
        kept_reading = std::fs::read_to_string(log.path()).unwrap().contains("after-input-marker");
    }
    let failed = store.get_events(task.id.as_str()).unwrap().into_iter().find(|event| {
        event.detail.contains("not reading terminal input")
    });
    unsafe { libc::kill(child_pid, libc::SIGKILL) };
    // A write blocked on a PTY may never wake, even after the child dies.
    let survived = done_rx.recv_timeout(Duration::from_secs(10));
    assert_eq!(survived.ok(), Some(true), "monitor must finish and survive undelivered input");
    assert!(kept_reading, "monitor must keep reading output after a blocked input write");
    assert_eq!(failed.map(|event| event.event_kind), Some(EventKind::Error));
    let messages = store.list_messages_for_task(task.id.as_str()).unwrap();
    assert!(messages[0].delivered_at.is_some() && messages[1].delivered_at.is_none());
}

#[test]
fn grok_output_file_contains_markdown_instead_of_json_envelope() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let envelope = r##"{"text":"# Findings\n\nThe report is rendered markdown."}"##;

    write_output_file(AgentKind::Grok, file.path().to_str().unwrap(), envelope).unwrap();

    assert_eq!(
        std::fs::read_to_string(file.path()).unwrap(),
        "# Findings\n\nThe report is rendered markdown."
    );
}
