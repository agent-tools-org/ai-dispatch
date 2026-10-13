// Stop settlement regressions using a real worker and the background reaper.
// Checks attribution at signal delivery and recovery after an interrupted stop.
// Deps: stop command, isolated AID home, shell subprocess and Store.

use super::{stop, tests::make_task};
use crate::{background, paths, store::Store, types::TaskStatus};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::Arc;

pub(super) fn save_worker(id: &str, pid: u32) {
    let mut spec: background::BackgroundRunSpec =
        serde_json::from_str(include_str!("../../../testdata/legacy-background-spec.json")).expect("spec");
    spec.task_id = id.to_string();
    spec.worker_pid = Some(pid);
    spec.agent_pid = None;
    spec.max_duration_secs = Some(3600);
    spec.idle_timeout_secs = Some(3600);
    background::save_spec(&spec).expect("save spec");
}

fn signal_observation(reap: bool) {
    let _permit = crate::test_subprocess::acquire();
    let home = tempfile::tempdir().expect("home");
    let _guard = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    store
        .insert_task(&make_task("t-stop-signal", TaskStatus::Running))
        .expect("insert");
    let (mut worker, mut output) = waiting_worker();
    let mut line = String::new();
    save_worker("t-stop-signal", worker.id());
    let observer_store = Arc::clone(&store);
    let observer_home = home.path().to_path_buf();
    let observer = std::thread::spawn(move || {
        let _guard = paths::AidHomeGuard::set(&observer_home);
        line.clear();
        output.read_line(&mut line).expect("signal");
        let snapshot = observer_store
            .get_task("t-stop-signal")
            .expect("get")
            .expect("task");
        let events = observer_store.get_events("t-stop-signal").expect("events");
        if let Err(err) = worker.wait() {
            assert_eq!(err.raw_os_error(), Some(libc::ECHILD), "worker reap: {err}");
        }
        if reap {
            background::check_zombie_tasks_with(&observer_store, |_| false).expect("reaper");
        }
        (snapshot, events, line)
    });
    stop(&store, "t-stop-signal").expect("stop");
    let (snapshot, events, line) = observer.join().expect("observer");
    assert_eq!(line.trim(), "signalled");
    assert_eq!(snapshot.status, TaskStatus::Stopped, "settle before signal");
    assert!(snapshot.completed_at.is_some());
    assert!(snapshot.duration_ms.is_some());
    assert_eq!(events.len(), 1, "event committed before signal");
    assert_eq!(events[0].detail, "Task stopped by user");
    assert_eq!(
        store
            .get_task("t-stop-signal")
            .expect("get")
            .expect("task")
            .status,
        TaskStatus::Stopped
    );
    assert_eq!(store.get_events("t-stop-signal").expect("events").len(), 1);
}

fn waiting_worker() -> (std::process::Child, BufReader<std::process::ChildStdout>) {
    let mut worker = Command::new("sh")
        .args([
            "-c",
            "trap 'echo signalled; exit 0' TERM; echo ready; while :; do sleep 0.1; done",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .expect("worker");
    let mut output = BufReader::new(worker.stdout.take().expect("stdout"));
    let mut line = String::new();
    output.read_line(&mut line).expect("ready");
    assert_eq!(line.trim(), "ready");
    (worker, output)
}

#[test]
fn settle_first_stop_is_committed_when_worker_receives_signal() {
    signal_observation(false);
}

#[test]
fn settle_first_reaper_after_signal_cannot_replace_stop_with_failure() {
    signal_observation(true);
}

#[test]
fn settle_first_reaper_snapshot_before_stop_cannot_record_failure() {
    let home = tempfile::tempdir().expect("home");
    let _guard = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    store
        .insert_task(&make_task("t-stop-reaper", TaskStatus::Running))
        .expect("insert");
    save_worker("t-stop-reaper", 999999);
    let cleaned = background::check_zombie_tasks_with(&store, |_| {
        stop(&store, "t-stop-reaper").expect("stop before reaper write");
        false
    })
    .expect("reaper");
    assert!(cleaned.is_empty());
    let task = store.get_task("t-stop-reaper").expect("get").expect("task");
    assert_eq!(task.status, TaskStatus::Stopped);
    assert!(task.completed_at.is_some());
    assert!(task.duration_ms.is_some());
    assert_eq!(store.get_events("t-stop-reaper").expect("events").len(), 1);
}

#[test]
fn settle_first_restop_signals_live_worker_without_duplicate_event() {
    let _permit = crate::test_subprocess::acquire();
    let home = tempfile::tempdir().expect("home");
    let _guard = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    store
        .insert_task(&make_task("t-stop-interrupted", TaskStatus::Pending))
        .expect("insert");
    stop(&store, "t-stop-interrupted").expect("settle");
    let before = store.get_task("t-stop-interrupted").expect("get").expect("task");
    let mut worker = Command::new("sleep").arg("30").spawn().expect("worker");
    save_worker("t-stop-interrupted", worker.id());
    let result = stop(&store, "t-stop-interrupted");
    let live = crate::test_subprocess::is_live(worker.id() as i32);
    let _ = worker.kill();
    if let Err(err) = worker.wait() {
        assert_eq!(err.raw_os_error(), Some(libc::ECHILD), "worker reap: {err}");
    }
    result.expect("repeat stop must recover signal delivery");
    assert!(!live, "repeat stop must kill the worker");
    let after = store.get_task("t-stop-interrupted").expect("get").expect("task");
    assert_eq!(before.completed_at, after.completed_at);
    assert_eq!(before.duration_ms, after.duration_ms);
    assert_eq!(store.get_events("t-stop-interrupted").expect("events").len(), 1);
}
