// Production entry point regression tests split to keep the parent within its size limit.
// Deps: parent test fixtures and production APIs.

use super::*;

/// t-73618fc7 / t-dcbd359e: a child that never reads its tty blocked the second
/// steer's PTY write and the monitor stopped reading output for 20 minutes. Claude
/// no longer takes PTY input, so an input-accepting adapter stands in for it.
#[test]
fn input_to_child_that_never_reads_does_not_wedge_monitor() {
    let _permit = crate::support::test_subprocess::acquire();
    let temp_home = tempfile::tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp_home.path());
    let task = super::super::tests::pty_task("t-unread-tty", TaskStatus::Running);
    let store = Arc::new(Store::open_memory().unwrap());
    store.insert_task(&task).unwrap();
    let second = format!("{}\n", "b".repeat(99)).repeat(1_400);
    for content in ["a".repeat(852), second] {
        store
            .insert_message(
                task.id.as_str(),
                MessageDirection::In,
                &content,
                MessageSource::Steer,
            )
            .unwrap();
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
        let mut state = MonitorState::with_policy(
            true,
            None,
            crate::config::timeout_policy::TimeoutPolicy::default(),
        );
        let result = monitor_bridge(
            &CursorAgent,
            &task_id,
            &monitor_store,
            &mut bridge,
            &receiver,
            &mut log_file,
            &mut state,
            None,
            None,
            None,
        );
        let _ = bridge.wait();
        let _ = done_tx.send(result.is_ok());
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let mut kept_reading = false;
    while !kept_reading && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
        kept_reading = std::fs::read_to_string(log.path())
            .unwrap()
            .contains("after-input-marker");
    }
    let failed = store
        .get_events(task.id.as_str())
        .unwrap()
        .into_iter()
        .find(|event| event.detail.contains("not reading terminal input"));
    unsafe { libc::kill(child_pid, libc::SIGKILL) };
    // A write blocked on a PTY may never wake, even after the child dies.
    let survived = done_rx.recv_timeout(Duration::from_secs(10));
    assert_eq!(
        survived.ok(),
        Some(true),
        "monitor must finish and survive undelivered input"
    );
    assert!(
        kept_reading,
        "monitor must keep reading output after a blocked input write"
    );
    assert_eq!(failed.map(|event| event.event_kind), Some(EventKind::Milestone));
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
