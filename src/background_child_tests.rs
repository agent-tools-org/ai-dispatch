// Background reaper regressions for descendant activity and first-token silence.
// Runs repeated probes to distinguish CPU growth from an unchanged living child.
// Deps: orphan reaper fixtures, ProcessGuard and temporary task state.

use super::tests::{insert_event, make_spec, make_task};
use super::*;
use std::process::{Command, Stdio};
use std::time::Duration;

fn reaped(script: &str, saw_output: bool) -> bool {
    let _permit = crate::support::test_subprocess::acquire();
    let temp = tempfile::tempdir().expect("temp");
    let _home = crate::support::paths::AidHomeGuard::set(temp.path());
    crate::support::paths::ensure_dirs().expect("task directories");
    let store = Store::open_memory().expect("store");
    let task = make_task("t-child-reaper");
    store.insert_task(&task).expect("task");
    let mut command = Command::new("sh");
    command.args(["-c", script]).stdout(Stdio::null()).stderr(Stdio::null());
    let guard = crate::process_guard::ProcessGuard::spawn(&mut command).expect("spawn");
    let mut spec = make_spec(task.id.as_str(), Some(guard.id()), Some(1));
    spec.agent_pid = Some(guard.id());
    spec.agent_started = process_monitor::activity::root_started(guard.id());
    spec.env = Some(std::collections::HashMap::from([("AID_FIRST_TOKEN_TIMEOUT_SECS".into(), "1".into())]));
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

#[test]
fn detached_reused_root_cannot_refresh_liveness_even_with_cached_activity() {
    let _permit = crate::support::test_subprocess::acquire();
    let temp = tempfile::tempdir().expect("temp");
    let _home = crate::support::paths::AidHomeGuard::set(temp.path());
    crate::support::paths::ensure_dirs().expect("dirs");
    let mut command = Command::new("sh");
    command.args(["-c", "sleep 10 & wait"]).stdout(Stdio::null()).stderr(Stdio::null());
    let root = crate::process_guard::ProcessGuard::spawn(&mut command).expect("replacement root");
    let store = Store::open_memory().expect("store");
    let task = make_task("t-reused-root");
    store.insert_task(&task).expect("task");
    insert_event(&store, task.id.as_str(), 10, EventKind::Reasoning);
    let mut spec = make_spec(task.id.as_str(), Some(root.id()), Some(1));
    spec.agent_pid = Some(root.id());
    let mut json = serde_json::to_value(&spec).expect("spec");
    json["agent_started"] = serde_json::json!("original-root-launch-identity");
    let spec = serde_json::from_value(json).expect("stale spec");
    std::thread::sleep(Duration::from_millis(100));
    let _ = process_monitor::activity::latest(task.id.as_str(), Some(root.id()), Duration::from_secs(60));
    let mut activity = latest_activity(&store, &task).expect("activity");
    let original = activity.timestamp;
    include_child_activity(task.id.as_str(), Some(&spec), 60, &mut activity);
    assert_eq!(activity.timestamp, original, "replacement tree must not refresh a stale task");
}

#[test]
fn detached_launch_persists_root_start_identity_with_pid() {
    let _permit = crate::support::test_subprocess::acquire();
    let temp = tempfile::tempdir().expect("temp");
    let _home = crate::support::paths::AidHomeGuard::set(temp.path());
    let mut command = Command::new("sleep");
    command.arg("10");
    let root = crate::process_guard::ProcessGuard::spawn(&mut command).expect("root");
    let spec = make_spec("t-root-identity", None, Some(1));
    super::super::background_spec::save_spec(&spec).expect("spec");
    super::super::background_process::update_agent_pid(&spec.task_id, root.id()).expect("launch");
    let json: serde_json::Value = serde_json::from_slice(
        &std::fs::read(crate::support::paths::job_path(&spec.task_id)).expect("persisted spec"),
    )
    .expect("json");
    assert_eq!(json["agent_pid"].as_u64(), Some(u64::from(root.id())));
    assert_eq!(json["agent_started"].as_str(), process_monitor::activity::root_started(root.id()).as_deref());
    assert!(json["agent_started"].as_str().is_some_and(|value| !value.is_empty()));
}
