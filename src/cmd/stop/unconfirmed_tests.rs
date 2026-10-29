// Unverifiable live agent identities never receive signals or worker settlement.
// The task status still becomes Stopped, and its owner retains a live, waitable child.
use super::*;
use crate::runtime::process::guard::ProcessGuard;
use crate::support::test_env::TestHome;
use std::process::Command;

#[test]
fn caller_owned_stop_json_missing_identity_is_unconfirmed_without_signals() {
    let _permit = crate::support::test_subprocess::acquire();
    for terminal in [false, true] {
        let (_home, store) = TestHome::with_dirs().shared_store();
        let status = if terminal {
            TaskStatus::Failed
        } else {
            TaskStatus::Running
        };
        let mut task = Task::test_fixture(TaskId("t-unconfirmed-stop".into()), status);
        task.completed_at = terminal.then(chrono::Local::now);
        store.insert_task(&task).expect("task");
        let mut command = Command::new("sleep");
        command.arg("60");
        let mut child = ProcessGuard::spawn(&mut command).expect("agent");
        let spec = background::BackgroundRunSpec {
            task_id: task.id.to_string(),
            worker_pid: Some(child.id()),
            agent_pid: Some(child.id()),
            agent_started: None,
            ..Default::default()
        };
        background::save_spec(&spec).expect("spec");
        assert_eq!(
            stop_json(&store, task.id.as_str(), false).expect("stop receipt"),
            5
        );
        assert!(
            child.child_mut().try_wait().expect("owner wait").is_none(),
            "foreign child alive"
        );
        let row = store.get_task(task.id.as_str()).expect("task").expect("row");
        assert_eq!(row.status, if terminal { status } else { TaskStatus::Stopped });
        let facts = store
            .execution_facts(task.id.as_str())
            .expect("facts")
            .expect("row");
        let worker = facts.settlement.expect("worker state");
        assert_eq!(worker.state, "unconfirmed");
        assert_eq!(worker.pid, Some(child.id()));
        assert!(worker.outcome.is_none());
        assert!(worker.settled_at.is_none());
        assert!(
            background::load_spec_if_exists(task.id.as_str())
                .expect("spec")
                .is_some()
        );
        assert!(row.final_head_sha.is_none());
    }
}

#[test]
fn stop_exit_confirmation_leaves_exited_recorded_child_waitable_by_its_owner() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store) = TestHome::with_dirs().shared_store();
    let task = Task::test_fixture(TaskId("t-stop-owner-wait".into()), TaskStatus::Running);
    store.insert_task(&task).expect("task");
    let mut command = Command::new("sh");
    command.args(["-c", "exit 7"]);
    let mut child = ProcessGuard::spawn(&mut command).expect("recorded child");
    super::settle_first_tests::save_worker(task.id.as_str(), child.id());
    assert!(
        crate::support::test_subprocess::wait_dead(child.id() as i32, std::time::Duration::from_secs(5)),
        "child exited without owner reaping"
    );
    stop(&store, task.id.as_str()).expect("dead worker stop");
    assert_eq!(
        child
            .child_mut()
            .wait()
            .expect("only the owner reaps the child")
            .code(),
        Some(7)
    );
    let facts = store
        .execution_facts(task.id.as_str())
        .expect("facts")
        .expect("row");
    assert_eq!(
        facts.settlement.expect("settled").outcome.as_deref(),
        Some("stopped")
    );
}
