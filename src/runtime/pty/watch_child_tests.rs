// PTY watchdog regressions with silent CPU tools and nonprogressing descendants.
// Exercises the live bridge with first-token progress already established.
// Deps: PTY monitor, fake streaming agent, temporary home and in-memory Store.

use super::*;
use std::io::Read;

fn monitor(script: &str) -> TaskStatus {
    monitor_with_first_token(script, false)
}

fn monitor_with_first_token(script: &str, first_token: bool) -> TaskStatus {
    let _permit = crate::support::test_subprocess::acquire();
    let _temp = crate::support::test_env::TestHome::new();
    let store = Arc::new(Store::open_memory().expect("store"));
    let task = super::tests::pty_task("t-child-pty", TaskStatus::Running);
    store.insert_task(&task).expect("task");
    let cmd = vec!["sh".into(), "-c".into(), script.into()];
    let mut bridge = PtyBridge::spawn(&cmd, None, vec![]).expect("bridge");
    let mut reader = bridge.take_reader().expect("reader");
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut bytes = [0; 1024];
        while let Ok(count) = reader.read(&mut bytes) {
            if count == 0 || tx.send(bytes[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut state = MonitorState::new(true, None);
    state.event_count = if first_token { 1 } else { 2 };
    state.received_raw_bytes = !first_token;
    let mut log = tempfile::tempfile().expect("log");
    monitor_bridge(
        &crate::runtime::watcher::idle_tests::FakeAgent,
        &task.id,
        &store,
        &mut bridge,
        &rx,
        &mut log,
        &mut state,
        first_token.then_some(Duration::from_millis(600)),
        Some(Duration::from_millis(600)),
        None,
    )
    .expect("monitor");
    if state.info.status == TaskStatus::Failed {
        bridge.kill_group().expect("kill");
    }
    bridge.wait().expect("exit");
    reader.join().expect("reader exit");
    state.info.status
}

#[test]
fn pty_silent_cpu_descendant_survives_idle_window() {
    assert_eq!(
        monitor("yes >/dev/null & tool=$!; sleep 2; kill $tool; exit 0"),
        TaskStatus::Done
    );
}

#[test]
fn pty_sleeping_descendant_is_reaped_after_idle_window() {
    assert_eq!(monitor("sleep 10 & wait"), TaskStatus::Failed);
}

#[test]
fn process_safety_pty_silent_first_cpu_command_survives_first_token_window() {
    assert_eq!(
        monitor_with_first_token(
            "yes >/dev/null & tool=$!; sleep 2; kill $tool; exit 0",
            true
        ),
        TaskStatus::Done
    );
}

#[test]
fn process_safety_pty_silent_first_sleeping_command_exhausts_first_token_window() {
    assert_eq!(
        monitor_with_first_token("sleep 10 & wait", true),
        TaskStatus::Failed
    );
}
