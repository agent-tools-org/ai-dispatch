// Background reaper regressions for descendant activity and first-token silence.
// Runs repeated probes to distinguish CPU growth from an unchanged living child.
// Deps: orphan reaper fixtures, ProcessGuard and temporary task state.

use super::tests::{insert_event, make_spec, make_task};
use super::*;
use std::process::{Command, Stdio};
use std::time::Duration;

fn reaped(script: &str, saw_output: bool) -> bool {
    let _permit = crate::test_subprocess::acquire();
    let temp = tempfile::tempdir().expect("temp");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().expect("task directories");
    let store = Store::open_memory().expect("store");
    let task = make_task("t-child-reaper");
    store.insert_task(&task).expect("task");
    let mut command = Command::new("sh");
    command
        .args(["-c", script])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let guard = crate::process_guard::ProcessGuard::spawn(&mut command).expect("spawn");
    let mut spec = make_spec(task.id.as_str(), Some(guard.id()), Some(1));
    spec.agent_pid = Some(guard.id());
    spec.env = Some(std::collections::HashMap::from([(
        "AID_FIRST_TOKEN_TIMEOUT_SECS".into(),
        "1".into(),
    )]));
    if saw_output {
        insert_event(&store, task.id.as_str(), 10, EventKind::Reasoning);
    }
    std::thread::sleep(Duration::from_millis(100));
    let activity = latest_activity(&store, &task).expect("activity");
    let mut activity_with_child = activity;
    include_child_activity(task.id.as_str(), Some(&spec), 1, &mut activity_with_child);
    std::thread::sleep(Duration::from_millis(2200));
    cleanup_wedged_live_worker(&store, &task, &spec, guard.id()).expect("reap")
}

#[test]
fn background_cpu_descendant_survives_idle_window() {
    assert!(!reaped("yes >/dev/null & wait", true));
}

#[test]
fn background_sleeping_descendant_is_reaped_after_idle_window() {
    assert!(reaped("sleep 10 & wait", true));
}

#[test]
fn background_cpu_descendant_does_not_extend_first_token_timeout() {
    assert!(reaped("yes >/dev/null & wait", false));
}
