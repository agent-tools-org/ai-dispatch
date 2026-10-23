// Stop command regressions for stale and changing worker process identities.
// Exercises production termination with owned subprocesses and isolated task state.

use super::{kill, settle_first_tests::save_worker, stop, tests::make_task};
use crate::{
    runtime::process::guard::ProcessGuard, store::Store, support::paths, types::TaskStatus,
};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::Arc;

fn with_task(f: impl FnOnce(&Arc<Store>, &str)) {
    let _permit = crate::support::test_subprocess::acquire();
    let home = tempfile::tempdir().expect("home");
    let _guard = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = "t-stop-identity";
    let mut task = make_task(id, TaskStatus::Running);
    task.agent = crate::types::AgentKind::Grok;
    store.insert_task(&task).expect("task");
    f(&store, id);
    assert_eq!(
        store.get_task(id).expect("get").expect("task").status,
        TaskStatus::Stopped
    );
    assert!(
        crate::runtime::background::load_spec_if_exists(id)
            .expect("spec")
            .is_none()
    );
}

fn worker(id: &str, on_term: &str) -> ProcessGuard {
    let mut command = Command::new("sh");
    command.args([
        "-c",
        "trap \"$2\" TERM; echo ready; while :; do sleep 0.1; done",
        "__run-task",
        id,
        on_term,
    ]);
    command.stdout(Stdio::piped());
    let mut worker = ProcessGuard::spawn(&mut command).expect("worker");
    let mut output = BufReader::new(worker.child_mut().stdout.take().expect("stdout"));
    let mut ready = String::new();
    output.read_line(&mut ready).expect("ready");
    assert_eq!(ready.trim(), "ready");
    worker
}

#[test]
fn stop_and_force_leave_unrelated_worker_pid_alive() {
    for action in [stop, kill] {
        with_task(|store, id| {
            let mut command = Command::new("sleep");
            command.arg("60");
            let mut child = ProcessGuard::spawn(&mut command).expect("unrelated child");
            save_worker(id, child.id());
            action(store, id).expect("terminate");
            assert!(
                child.child_mut().try_wait().expect("wait").is_none(),
                "unrelated child survived"
            );
        });
    }
}

#[test]
fn stop_and_force_signal_matching_worker() {
    for action in [stop, kill] {
        with_task(|store, id| {
            let child = worker(id, "trap - TERM; kill -TERM $$");
            save_worker(id, child.id());
            action(store, id).expect("terminate");
            assert!(
                !crate::support::test_subprocess::is_live(child.id() as i32),
                "matching worker must be signalled"
            );
        });
    }
}

#[test]
fn stop_rechecks_worker_identity_before_sigkill_escalation() {
    with_task(|store, id| {
        let mut child = worker(id, "exec sleep 60");
        save_worker(id, child.id());
        stop(store, id).expect("stop");
        assert!(
            child.child_mut().try_wait().expect("wait").is_none(),
            "replacement survived escalation"
        );
    });
}
