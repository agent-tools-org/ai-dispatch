// Launch evidence is caller-owned only; the worker retains its lifecycle barrier.
// Tests use real SQLite guards and owned process fixtures without reaping through probes.
use super::*;
use crate::{runtime::background, types::*};

fn fixture() -> (crate::support::test_env::TestHome, std::sync::Arc<Store>, TaskId) {
    let (home, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    let id = TaskId("t-round7-launch".into());
    store
        .insert_task(&Task::test_fixture(id.clone(), TaskStatus::Running))
        .expect("task");
    (home, store, id)
}

#[test]
fn default_launches_never_write_execution_evidence_or_read_policy() {
    let (_home, store, id) = fixture();
    store
        .db()
        .execute_batch(
            "CREATE TRIGGER reject_evidence BEFORE UPDATE OF
        agent_process,worker_spawned ON tasks BEGIN SELECT RAISE(ABORT,'unexpected evidence'); END;",
        )
        .expect("reject evidence writes");
    store
        .db()
        .execute(
            "UPDATE tasks SET execution_policy='invalid-json' WHERE id=?1",
            [id.as_str()],
        )
        .expect("unreadable policy");
    let before = store
        .db()
        .query_row("SELECT revision FROM tasks WHERE id=?1", [id.as_str()], |row| {
            row.get::<_, i64>(0)
        })
        .expect("revision");
    for _ in 0..2 {
        store
            .with_worker_launch(false, id.as_str(), || Ok(()))
            .expect("default worker");
        store
            .with_running_task(false, id.as_str(), || Ok(()))
            .expect("default agent");
    }
    let after = store
        .db()
        .query_row("SELECT revision FROM tasks WHERE id=?1", [id.as_str()], |row| {
            row.get::<_, i64>(0)
        })
        .expect("revision");
    assert_eq!(before, after, "guard no-op must not advance revision");
}

#[test]
fn revision_does_not_advance_for_any_unchanged_lifecycle_field() {
    let (_home, store, id) = fixture();
    let before = store.execution_snapshot(id.as_str()).expect("snapshot");
    store
        .db()
        .execute(
            "UPDATE tasks SET status=status,agent=agent,custom_agent_name=custom_agent_name,
        execution_policy=execution_policy,worker_settlement=worker_settlement,model=model,
        observed_model=observed_model,attribution_source=attribution_source,exit_code=exit_code,
        completed_at=completed_at,agent_process=agent_process,worker_spawned=worker_spawned,
        execution_stop=execution_stop WHERE id=?1",
            [id.as_str()],
        )
        .expect("no-op");
    assert_eq!(store.execution_snapshot(id.as_str()).expect("snapshot"), before);
}

#[cfg(unix)]
#[test]
fn caller_owned_refused_launch_keeps_a_live_workers_lifecycle_barrier() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, id) = fixture();
    let mut command = std::process::Command::new("sh");
    command
        .args(["-c", "read line", "__run-task", id.as_str()])
        .stdin(std::process::Stdio::piped());
    let _worker = crate::runtime::process::guard::ProcessGuard::spawn(&mut command).expect("worker");
    background::save_spec(
        &serde_json::from_value(serde_json::json!({
        "task_id":id.as_str(),"worker_pid":_worker.id()}))
        .expect("spec"),
    )
    .expect("save");
    store
        .update_task_status(id.as_str(), TaskStatus::Failed)
        .expect("failed during lifecycle");
    assert!(
        store
            .with_running_task(true, id.as_str(), || -> Result<()> {
                panic!("guard refuses before spawn");
            })
            .is_err()
    );
    assert!(crate::support::paths::job_path(id.as_str()).exists());
    let receipt = crate::cmd::execution_receipt::receipt(&store, id.as_str()).expect("receipt");
    assert_eq!(receipt["worker"]["state"], "running");
    assert_eq!(receipt["settled"], false);
}

#[test]
fn caller_owned_spawn_failure_resets_evidence_and_derives_launch_failed() {
    let (_home, store, id) = fixture();
    let result = store.with_running_task(true, id.as_str(), || {
        std::process::Command::new("/definitely/missing/aid-agent")
            .spawn()
            .map_err(|error| background::AgentSpawnFailure(error.into()).into())
    });
    assert!(result.is_err());
    assert_eq!(
        store
            .execution_process(id.as_str())
            .expect("facts")
            .0
            .expect("agent")
            .state,
        "not_started"
    );
    assert!(
        crate::support::paths::job_path(id.as_str()).exists(),
        "worker owns removal"
    );
    store
        .update_task_status(id.as_str(), TaskStatus::Failed)
        .expect("failed");
    background::clear_spec(id.as_str()).expect("worker finishes cleanup");
    let receipt = crate::cmd::execution_receipt::receipt(&store, id.as_str()).expect("receipt");
    assert_eq!(receipt["worker"]["outcome"], "launch_failed");
}

#[test]
fn caller_owned_evidence_setup_waits_for_the_worker_launch_transaction() {
    use std::sync::mpsc;
    use std::time::Duration;
    let home = crate::support::test_env::TestHome::with_dirs();
    let path = home.path().join("launch-lock.db");
    let store = Store::open(&path).expect("store");
    store
        .insert_task(&Task::test_fixture(
            TaskId("t-launch-lock".into()),
            TaskStatus::Running,
        ))
        .expect("row");
    let worker_store = Store::open(&path).expect("worker store");
    let conn = store.db();
    let tx = conn.unchecked_transaction().expect("launcher");
    tx.execute("UPDATE tasks SET status=status WHERE id='t-launch-lock'", [])
        .expect("guard");
    let (ready, started) = mpsc::channel();
    let (done, result) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let store = worker_store;
        ready.send(()).expect("starting");
        done.send(store.begin_agent_launch("t-launch-lock"))
            .expect("result");
    });
    started
        .recv_timeout(Duration::from_secs(5))
        .expect("worker loaded");
    assert!(matches!(
        result.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    tx.commit().expect("publish worker");
    result
        .recv_timeout(Duration::from_secs(5))
        .expect("launch result")
        .expect("no read/write upgrade failure");
    worker.join().expect("worker");
}

#[test]
fn v6_revision_triggers_upgrade_without_bumping_for_no_op_status_writes() {
    let home = crate::support::test_env::TestHome::with_dirs();
    let path = home.path().join("v6-upgrade.db");
    let store = Store::open(&path).expect("store");
    store
        .insert_task(&Task::test_fixture(
            TaskId("t-v6-noop".into()),
            TaskStatus::Running,
        ))
        .expect("task");
    store
        .db()
        .execute_batch(
            "DROP TRIGGER execution_change_revision;
        CREATE TRIGGER execution_change_revision AFTER UPDATE OF status ON tasks BEGIN
          INSERT INTO execution_revisions(id) VALUES(NULL);
          UPDATE tasks SET revision=last_insert_rowid() WHERE id=NEW.id;
          DELETE FROM execution_revisions;
        END;
        PRAGMA user_version=6;",
        )
        .expect("old unconditional trigger");
    drop(store);
    let store = Store::open(&path).expect("upgrade v6");
    let before = store.execution_snapshot("t-v6-noop").expect("snapshot");
    store
        .db()
        .execute("UPDATE tasks SET status=status WHERE id='t-v6-noop'", [])
        .expect("guard");
    assert_eq!(store.execution_snapshot("t-v6-noop").expect("snapshot"), before);
}
