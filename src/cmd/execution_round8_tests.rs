// Default JSON admission is read-only; owned workers can use row PID evidence.
// Uses owned process fixtures and malformed specs to expose accidental probes or signals.
use super::*;
use crate::{support::test_env::TestHome, types::*};

#[tokio::test]
async fn default_json_wait_and_stop_refuse_without_observing_or_writing() {
    let (_home, store) = TestHome::with_dirs().shared_store();
    let id = "t-default-json";
    store
        .insert_task(&Task::test_fixture(TaskId(id.into()), TaskStatus::Done))
        .expect("task");
    store
        .settle_worker(id, "launch_failed", None)
        .expect("old incorrect settlement");
    std::fs::write(crate::support::paths::job_path(id), "unreadable spec").expect("spec");
    let before = store.execution_snapshot(id).expect("snapshot");
    let changes = store.db().total_changes();
    let errors = [
        crate::cmd::wait::run_json(&store, &[id.into()], Some(0))
            .await
            .expect_err("wait refusal"),
        crate::cmd::stop::stop_json(&store, id, true).expect_err("stop refusal"),
    ];
    for error in errors {
        let refusal = error
            .downcast_ref::<crate::execution::Refusal>()
            .expect("typed refusal");
        assert_eq!(refusal.exit, 3);
        assert_eq!(refusal.code, "policy_unsupported");
        let body = refusal.body();
        assert!(body["policy"].is_null());
        assert_eq!(body["worker"]["state"], "unconfirmed");
        assert_eq!(body["settled"], false);
    }
    assert_eq!(
        read_only_receipt(&store, id).expect("show")["worker"]["state"],
        "unconfirmed"
    );
    assert_eq!(receipt(&store, id).expect("default receipt")["settled"], false);
    assert!(!derive_settlement(&store, id).expect("no derivation"));
    assert_eq!(store.execution_snapshot(id).expect("snapshot"), before);
    assert_eq!(store.db().total_changes(), changes);
    assert_eq!(
        std::fs::read_to_string(crate::support::paths::job_path(id)).expect("spec"),
        "unreadable spec"
    );
}

#[cfg(unix)]
#[test]
fn default_json_stop_leaves_a_live_worker_and_agent_untouched() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store) = TestHome::with_dirs().shared_store();
    let id = "t-default-live";
    store
        .insert_task(&Task::test_fixture(TaskId(id.into()), TaskStatus::Running))
        .expect("task");
    let mut command = std::process::Command::new("sh");
    command
        .args(["-c", "read line", "__run-task", id])
        .stdin(std::process::Stdio::piped());
    let mut worker = crate::runtime::process::guard::ProcessGuard::spawn(&mut command).expect("worker");
    let mut command = std::process::Command::new("sleep");
    command.arg("60");
    let mut agent = crate::runtime::process::guard::ProcessGuard::spawn(&mut command).expect("agent");
    background::save_spec(
        &serde_json::from_value(json!({"task_id":id,
        "worker_pid":worker.id(),"agent_pid":agent.id()}))
        .expect("spec"),
    )
    .expect("save");
    let before = store.execution_snapshot(id).expect("snapshot");
    assert_eq!(
        crate::cmd::stop::stop_json(&store, id, true)
            .expect_err("refused")
            .downcast_ref::<crate::execution::Refusal>()
            .expect("refusal")
            .exit,
        3
    );
    assert!(worker.child_mut().try_wait().expect("worker alive").is_none());
    assert!(agent.child_mut().try_wait().expect("agent alive").is_none());
    assert_eq!(store.execution_snapshot(id).expect("snapshot"), before);
}

#[cfg(unix)]
#[test]
fn caller_owned_wait_and_stop_use_the_known_pid_when_spec_publication_failed() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store) = TestHome::with_dirs().shared_store();
    let id = "t-row-worker-pid";
    store
        .insert_task(&Task::test_fixture(TaskId(id.into()), TaskStatus::Running))
        .expect("task");
    store.test_caller_owned(id);
    let mut command = std::process::Command::new("sh");
    command
        .args(["-c", "read line", "__run-task", id])
        .stdin(std::process::Stdio::piped());
    let mut worker = crate::runtime::process::guard::ProcessGuard::spawn(&mut command).expect("worker");
    background::save_spec(&serde_json::from_value(json!({"task_id":id})).expect("spec")).expect("save");
    store.mark_worker_spawned(id).expect("spawned");
    store
        .observe_worker_state(id, "running", Some(worker.id()), None)
        .expect("known PID");
    let receipt = receipt(&store, id).expect("row PID observation");
    assert_eq!(receipt["worker"]["state"], "running");
    assert_eq!(receipt["worker"]["pid"], worker.id());
    assert_eq!(
        crate::cmd::stop::stop_json(&store, id, true).expect("confirmed stop"),
        0
    );
    assert!(!worker.child_mut().wait().expect("only owner reaps").success());
}

#[test]
fn default_receipts_ignore_untrusted_worker_and_agent_metadata() {
    let (_home, store) = TestHome::with_dirs().shared_store();
    let id = "t-default-old-metadata";
    store
        .insert_task(&Task::test_fixture(TaskId(id.into()), TaskStatus::Done))
        .expect("task");
    store
        .db()
        .execute(
            "UPDATE tasks SET worker_settlement='invalid',agent_process='invalid' WHERE id=?1",
            [id],
        )
        .expect("legacy metadata");
    let changes = store.db().total_changes();
    let shown = read_only_receipt(&store, id).expect("default receipt");
    assert!(shown["policy"].is_null());
    assert_eq!(shown["worker"]["state"], "unconfirmed");
    assert_eq!(shown["settled"], false);
    assert_eq!(store.db().total_changes(), changes);
}

#[test]
fn unconfirmed_observation_and_stop_keep_the_known_worker_pid() {
    let (_home, store) = TestHome::with_dirs().shared_store();
    let id = "t-pid-unconfirmed";
    store
        .insert_task(&Task::test_fixture(TaskId(id.into()), TaskStatus::Failed))
        .expect("task");
    store.test_caller_owned(id);
    store
        .db()
        .execute(
            "UPDATE tasks SET agent_process=NULL,worker_spawned=1 WHERE id=?1",
            [id],
        )
        .expect("unavailable agent identity");
    background::save_spec(&serde_json::from_value(json!({"task_id":id})).expect("spec")).expect("save");
    store
        .observe_worker_state(id, "running", Some(u32::MAX), None)
        .expect("known worker");
    let observed = receipt(&store, id).expect("observation");
    assert_eq!(observed["worker"]["state"], "unconfirmed");
    assert_eq!(observed["worker"]["pid"], u32::MAX);
    assert_eq!(
        crate::cmd::stop::stop_json(&store, id, true).expect("unconfirmed stop"),
        5
    );
    let worker = store
        .execution_facts(id)
        .expect("facts")
        .expect("row")
        .settlement
        .expect("worker");
    assert_eq!(worker.state, "unconfirmed");
    assert_eq!(worker.pid, Some(u32::MAX));
}
