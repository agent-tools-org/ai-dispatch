// Caller-owned evidence publication failures abort and clean the actual spawned group.
// The owned PID is retained independently of the spec and the child remains owner-waitable.
use super::*;
use crate::{
    runtime::{background, process::guard::ProcessGuard},
    types::*,
};

#[cfg(unix)]
fn failed_publication(reject_write: bool) {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    let id = "t-publication-abort";
    store
        .insert_task(&Task::test_fixture(TaskId(id.into()), TaskStatus::Running))
        .expect("task");
    store.test_caller_owned(id);
    if reject_write {
        store
            .db()
            .execute_batch(
                "CREATE TRIGGER reject_agent BEFORE UPDATE OF agent_process ON tasks
            WHEN json_extract(NEW.agent_process,'$.state')='spawned'
            BEGIN SELECT RAISE(ABORT,'agent evidence unavailable'); END;",
            )
            .expect("reject evidence");
    }
    let mut owner = None;
    let result: Result<()> = store.with_running_task(true, id, |pid| {
        let mut command = std::process::Command::new("sleep");
        command.arg("60");
        let child = ProcessGuard::spawn(&mut command)?;
        *pid = Some((
            child.id(),
            crate::runtime::process::monitor::activity::root_started(child.id()),
        ));
        if reject_write {
            background::update_agent_pid(id, child.id())?;
        }
        owner = Some(child);
        Ok(())
    });
    assert!(result.is_err(), "missing PID and write failures must both abort");
    assert_aborted(&store, id, owner.expect("child"));
}

#[cfg(unix)]
fn assert_aborted(store: &Store, id: &str, mut owner: ProcessGuard) {
    assert!(crate::support::test_subprocess::wait_dead(
        owner.id() as i32,
        std::time::Duration::from_secs(5)
    ));
    assert!(!owner.child_mut().wait().expect("owner wait").success());
    assert_eq!(
        store
            .execution_process(id)
            .expect("evidence")
            .0
            .expect("agent")
            .state,
        "not_started"
    );
    assert!(
        crate::support::paths::job_path(id).exists(),
        "worker retains the barrier"
    );
}

#[cfg(unix)]
#[test]
fn caller_owned_evidence_write_failure_kills_the_spawned_agent_and_resets_evidence() {
    failed_publication(true);
}

#[cfg(unix)]
#[test]
fn caller_owned_missing_published_pid_kills_the_spawned_agent_and_resets_evidence() {
    failed_publication(false);
}

#[test]
fn v7_policy_trigger_upgrade_bumps_every_explicit_policy_write() {
    let home = crate::support::test_env::TestHome::with_dirs();
    let path = home.path().join("policy-upgrade.db");
    let store = Store::open(&path).expect("store");
    let id = "t-policy-upgrade";
    store
        .insert_task(&Task::test_fixture(TaskId(id.into()), TaskStatus::Running))
        .expect("task");
    store
        .db()
        .execute_batch("DROP TRIGGER execution_policy_revision; PRAGMA user_version=7;")
        .expect("simulate v7");
    drop(store);
    let store = Store::open(&path).expect("v8 migration");
    let mut revision = store.execution_facts(id).expect("facts").expect("row").revision;
    for _ in 0..2 {
        store
            .db()
            .execute("UPDATE tasks SET execution_policy=NULL WHERE id=?1", [id])
            .expect("policy write");
        let next = store.execution_facts(id).expect("facts").expect("row").revision;
        assert!(
            next > revision,
            "a policy write must advance the store-wide sequence"
        );
        revision = next;
    }
    store
        .db()
        .execute("UPDATE tasks SET status=status WHERE id=?1", [id])
        .expect("guard no-op");
    assert_eq!(
        store.execution_facts(id).expect("facts").expect("row").revision,
        revision
    );
}

#[cfg(unix)]
#[test]
fn stale_spec_pid_aborts_the_current_launch_without_signalling_the_foreign_process() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    let id = "t-stale-agent-publication";
    store
        .insert_task(&Task::test_fixture(TaskId(id.into()), TaskStatus::Running))
        .expect("task");
    store.test_caller_owned(id);
    let mut command = std::process::Command::new("sleep");
    command.arg("60");
    let mut foreign = ProcessGuard::spawn(&mut command).expect("foreign");
    background::save_spec(
        &serde_json::from_value(serde_json::json!({
        "task_id":id,"agent_pid":foreign.id()}))
        .expect("stale spec"),
    )
    .expect("save");
    let mut owner = None;
    let result: Result<()> = store.with_running_task(true, id, |identity| {
        let mut command = std::process::Command::new("sleep");
        command.arg("60");
        let child = ProcessGuard::spawn(&mut command)?;
        *identity = Some((
            child.id(),
            crate::runtime::process::monitor::activity::root_started(child.id()),
        ));
        owner = Some(child);
        Ok(())
    });
    assert!(result.is_err());
    assert_aborted(&store, id, owner.expect("actual child"));
    assert!(foreign.child_mut().try_wait().expect("foreign alive").is_none());
}
