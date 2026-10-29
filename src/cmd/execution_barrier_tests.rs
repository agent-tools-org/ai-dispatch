// Derived receipts depend on the existing worker spec and persisted agent group.
// Owned children remain waitable: observations never reap or kill them.
use super::*;
use crate::{
    runtime::{background, process::guard::ProcessGuard},
    support::test_env::TestHome,
    types::*,
};

pub(super) fn fixture() -> (TestHome, std::sync::Arc<Store>, TaskId) {
    let (home, store) = TestHome::with_dirs().shared_store();
    let id = TaskId("t-derived-barrier".into());
    store
        .insert_task(&Task::test_fixture(id.clone(), TaskStatus::Running))
        .expect("task");
    (home, store, id)
}

pub(super) fn record_agent(store: &Store, id: &TaskId, child: &ProcessGuard) {
    let agent = crate::store::execution_protocol::AgentProcess {
        state: "spawned".into(),
        pid: Some(child.id()),
        pgid: Some(child.id()),
        started: crate::runtime::process::monitor::activity::root_started(child.id()),
    };
    assert!(agent.started.is_some());
    store
        .db()
        .execute(
            "UPDATE tasks SET agent_process = ?2, worker_spawned = 1 WHERE id = ?1",
            rusqlite::params![id.as_str(), serde_json::to_string(&agent).expect("agent")],
        )
        .expect("record");
}

pub(super) fn live_agent() -> ProcessGuard {
    let mut command = std::process::Command::new("sleep");
    command.arg("60");
    ProcessGuard::spawn(&mut command).expect("agent")
}

#[test]
fn caller_owned_dead_worker_live_agent_never_settles() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, id) = fixture();
    let mut agent = live_agent();
    record_agent(&store, &id, &agent);
    background::save_spec(&background::BackgroundRunSpec {
        task_id: id.to_string(),
        worker_pid: Some(u32::MAX),
        ..Default::default()
    })
    .expect("barrier");
    assert_eq!(receipt(&store, id.as_str()).expect("observe")["settled"], false);
    assert!(agent.child_mut().try_wait().expect("owner wait").is_none());
    assert!(
        background::load_spec_if_exists(id.as_str())
            .expect("spec")
            .is_some()
    );
    agent.force_kill();
    agent.child_mut().wait().expect("owner wait");
    assert_eq!(
        receipt(&store, id.as_str()).expect("observe")["worker"]["outcome"],
        "lost"
    );
}

#[test]
fn caller_owned_spec_absent_live_agent_never_settles() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, id) = fixture();
    let mut agent = live_agent();
    record_agent(&store, &id, &agent);
    assert_eq!(receipt(&store, id.as_str()).expect("observe")["settled"], false);
    assert!(agent.child_mut().try_wait().expect("owner wait").is_none());
    agent.force_kill();
    agent.child_mut().wait().expect("owner wait");
    assert_eq!(
        receipt(&store, id.as_str()).expect("observe")["worker"]["outcome"],
        "completed"
    );
}

#[test]
fn caller_owned_agent_child_keeps_group_open_after_leader_exits() {
    use std::io::{BufRead, BufReader};
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, id) = fixture();
    let mut command = std::process::Command::new("sh");
    command
        .args(["-c", "sleep 60 & echo $!; read ignored"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    let mut agent = ProcessGuard::spawn(&mut command).expect("agent");
    let mut line = String::new();
    BufReader::new(agent.child_mut().stdout.take().expect("stdout"))
        .read_line(&mut line)
        .expect("child");
    let child: i32 = line.trim().parse().expect("pid");
    record_agent(&store, &id, &agent);
    agent.child_mut().kill().expect("kill leader only");
    agent.child_mut().wait().expect("reap leader");
    let observed = receipt(&store, id.as_str()).expect("observe");
    unsafe {
        libc::kill(child, libc::SIGKILL);
    }
    assert_eq!(
        observed["settled"], false,
        "live member survives a dead/reaped leader"
    );
    assert_eq!(observed["worker"]["state"], "unconfirmed");
}

#[test]
fn caller_owned_missing_pgid_is_unconfirmed_even_after_agent_exits() {
    let (_home, store, id) = fixture();
    store
        .db()
        .execute(
            "UPDATE tasks SET agent_process = ?2 WHERE id = ?1",
            rusqlite::params![
                id.as_str(),
                r#"{"state":"spawned","pid":4294967295,"started":"old"}"#
            ],
        )
        .expect("legacy evidence");
    let observed = receipt(&store, id.as_str()).expect("observe");
    assert_eq!(observed["settled"], false);
    assert_eq!(observed["worker"]["state"], "unconfirmed");
}

#[test]
fn caller_owned_missing_identity_is_unconfirmed_even_with_a_pgid() {
    let (_home, store, id) = fixture();
    store
        .db()
        .execute(
            "UPDATE tasks SET agent_process = ?2 WHERE id = ?1",
            rusqlite::params![
                id.as_str(),
                r#"{"state":"spawned","pid":4294967295,"pgid":2147483647}"#
            ],
        )
        .expect("legacy evidence");
    assert_eq!(
        receipt(&store, id.as_str()).expect("observe")["worker"]["state"],
        "unconfirmed"
    );
}

#[test]
fn caller_owned_mismatched_identity_never_skips_a_live_agent_group() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, id) = fixture();
    let mut agent = live_agent();
    record_agent(&store, &id, &agent);
    store
        .db()
        .execute(
            "UPDATE tasks SET agent_process = json_set(agent_process, '$.started', 'previous') WHERE id = ?1",
            [id.as_str()],
        )
        .expect("reused pid");
    assert_eq!(receipt(&store, id.as_str()).expect("observe")["settled"], false);
    assert!(agent.child_mut().try_wait().expect("owner wait").is_none());
}

#[test]
fn caller_owned_launch_failure_is_only_derived_by_observation() {
    let (_home, store, id) = fixture();
    store
        .update_task_status(id.as_str(), TaskStatus::Failed)
        .expect("failed launch");
    assert_ne!(
        store
            .execution_snapshot(id.as_str())
            .expect("snapshot")
            .expect("row")["worker"]["state"],
        "settled"
    );
    assert_eq!(
        receipt(&store, id.as_str()).expect("observe")["worker"]["outcome"],
        "launch_failed"
    );
}

#[test]
fn caller_owned_first_derived_settlement_wins() {
    let (_home, store, id) = fixture();
    store.mark_worker_spawned(id.as_str()).expect("spawned");
    let first = receipt(&store, id.as_str()).expect("completed");
    store.record_execution_stop(id.as_str()).expect("later stop");
    let second = receipt(&store, id.as_str()).expect("existing");
    assert_eq!(first["worker"], second["worker"]);
    assert_eq!(second["worker"]["outcome"], "completed");
}

#[test]
fn caller_owned_live_worker_keeps_existing_barrier_open() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, id) = fixture();
    let mut command = std::process::Command::new("sh");
    command.args(["-c", "sleep 60", "__run-task", id.as_str()]);
    let mut worker = ProcessGuard::spawn(&mut command).expect("worker");
    background::save_spec(&background::BackgroundRunSpec {
        task_id: id.to_string(),
        worker_pid: Some(worker.id()),
        ..Default::default()
    })
    .expect("spec");
    assert_eq!(
        receipt(&store, id.as_str()).expect("observe")["worker"]["state"],
        "running"
    );
    assert!(worker.child_mut().try_wait().expect("owner wait").is_none());
}

#[test]
fn caller_owned_unreadable_spec_remains_unconfirmed_without_deleting_it() {
    let (_home, store, id) = fixture();
    std::fs::write(crate::support::paths::job_path(id.as_str()), "not json").expect("spec");
    assert_eq!(
        receipt(&store, id.as_str()).expect("observe")["worker"]["state"],
        "unconfirmed"
    );
    assert_eq!(
        std::fs::read_to_string(crate::support::paths::job_path(id.as_str())).expect("spec"),
        "not json"
    );
}
