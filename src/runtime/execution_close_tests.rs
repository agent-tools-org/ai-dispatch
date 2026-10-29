// Settlement predicates cover containers, tool sessions, launch failure and competing closers.
// All probes retain ownership of children so the test can still wait for them.
use super::*;
use crate::{
    support::test_env::TestHome,
    types::{Task, TaskId, TaskStatus},
};

fn fixture(id: &str) -> (TestHome, std::sync::Arc<Store>, BackgroundRunSpec) {
    let (home, store) = TestHome::with_dirs().shared_store();
    store
        .insert_task(&Task::test_fixture(TaskId(id.into()), TaskStatus::Running))
        .expect("task");
    (
        home,
        store,
        BackgroundRunSpec {
            task_id: id.into(),
            ..Default::default()
        },
    )
}

#[test]
fn caller_owned_every_closer_rejects_a_live_recorded_container() {
    let (_home, store, mut spec) = fixture("t-container-close");
    spec.container_name = Some("aid-task-container".into());
    execution_processes::CONTAINER_LIST.with(|slot| {
        *slot.borrow_mut() = Some(serde_json::json!([
            {"configuration": {"id": "aid-task-container"}, "status": "running"}
        ]))
    });
    for caller in [
        Closer::Worker,
        Closer::Stop,
        Closer::Reaper,
        Closer::Observation,
        Closer::LaunchFailed,
    ] {
        assert!(matches!(
            close_execution(&store, &spec.task_id, "stopped", caller, Some(&spec)).expect("probe"),
            Closed::Unconfirmed(_)
        ));
        assert!(
            !store
                .execution_facts(&spec.task_id)
                .expect("facts")
                .expect("row")
                .settlement
                .is_some_and(|s| s.state == "settled")
        );
    }
    execution_processes::CONTAINER_LIST.with(|slot| *slot.borrow_mut() = None);
}

#[cfg(unix)]
#[test]
fn caller_owned_live_tool_session_blocks_close_and_owner_can_reap() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, mut spec) = fixture("t-tool-close");
    let mut command = std::process::Command::new("sleep");
    command.arg("60");
    let mut tool = crate::runtime::process::guard::ProcessGuard::spawn(&mut command).expect("tool");
    spec.worker_pid = Some(u32::MAX);
    spec.tool_sessions.push(execution_processes::RecordedProcess {
        pid: tool.id(),
        started: super::super::process::monitor::activity::root_started(tool.id()),
    });
    assert!(matches!(
        close_execution(&store, &spec.task_id, "lost", Closer::Observation, Some(&spec)).expect("probe"),
        Closed::Unconfirmed(_)
    ));
    assert!(tool.child_mut().try_wait().expect("owner wait").is_none());
    let retained = background::load_spec_if_exists(&spec.task_id)
        .expect("evidence")
        .expect("spec");
    assert_eq!(retained.tool_sessions[0].pid, tool.id());
    assert!(retained.tool_sessions[0].started.is_some());
    tool.force_kill();
    assert!(
        tool.child_mut().wait().is_ok(),
        "closer must never reap a recorded process"
    );
}

#[test]
fn caller_owned_launch_failure_cannot_close_any_recorded_worker_launch() {
    let (_home, store, mut spec) = fixture("t-launch-close");
    spec.worker_pid = Some(u32::MAX);
    assert!(matches!(
        close_execution(
            &store,
            &spec.task_id,
            "launch_failed",
            Closer::LaunchFailed,
            Some(&spec)
        )
        .expect("probe"),
        Closed::Unconfirmed(_)
    ));
}

#[test]
fn caller_owned_first_closer_wins_and_later_closer_reports_existing() {
    let (_home, store, spec) = fixture("t-first-close");
    assert_eq!(
        close_execution(&store, &spec.task_id, "completed", Closer::Worker, Some(&spec))
            .expect("worker close"),
        Closed::Written
    );
    let revision = store
        .execution_facts(&spec.task_id)
        .expect("facts")
        .expect("row")
        .revision;
    assert_eq!(
        close_execution(&store, &spec.task_id, "stopped", Closer::Stop, Some(&spec)).expect("stop close"),
        Closed::Existing
    );
    let facts = store.execution_facts(&spec.task_id).expect("facts").expect("row");
    assert_eq!(facts.revision, revision);
    assert_eq!(
        facts.settlement.expect("worker").outcome.as_deref(),
        Some("completed")
    );
}

#[cfg(unix)]
#[test]
fn caller_owned_worker_exemption_applies_only_to_the_current_process() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, mut spec) = fixture("t-other-worker");
    let mut command = std::process::Command::new("sh");
    command.args(["-c", "sleep 60", "__run-task", &spec.task_id]);
    let mut worker = crate::runtime::process::guard::ProcessGuard::spawn(&mut command).expect("worker");
    spec.worker_pid = Some(worker.id());
    assert!(matches!(
        close_execution(&store, &spec.task_id, "completed", Closer::Worker, Some(&spec)).expect("confirm"),
        Closed::Unconfirmed(_)
    ));
    assert!(worker.child_mut().try_wait().expect("owner wait").is_none());
}
