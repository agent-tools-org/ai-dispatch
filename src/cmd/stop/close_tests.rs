// Protocol stop covers the recorded worker/agent group; default text stays on main's path.
// Real process guards verify group signals and retain the owner's wait responsibility.
use super::*;
use crate::{support::test_env::TestHome, runtime::process::guard::ProcessGuard};

pub(super) fn agent_fixture() -> (TestHome, Arc<Store>, Task, ProcessGuard) {
    let (home, store) = TestHome::with_dirs().shared_store();
    let task = Task::test_fixture(TaskId("t-protocol-stop".into()), TaskStatus::Running);
    store.insert_task(&task).expect("task");
    let agent = store
        .with_running_task(true, task.id.as_str(), || {
            let mut command = std::process::Command::new("sleep");
            command.arg("60");
            let agent = ProcessGuard::spawn(&mut command)?;
            background::update_agent_pid(task.id.as_str(), agent.id())?;
            Ok(agent)
        })
        .expect("agent launch");
    (home, store, task, agent)
}

#[test]
fn caller_owned_stop_cleanup_failure_never_writes_settlement() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, task, mut agent) = agent_fixture();
    let repo = crate::support::test_repo::repo_with_file("base.txt", "base\n", "base");
    store
        .db()
        .execute(
            "UPDATE tasks SET worktree_path = ?2 WHERE id = ?1",
            rusqlite::params![task.id.as_str(), repo.path().display().to_string()],
        )
        .expect("worktree");
    store
        .db()
        .execute_batch(
            "CREATE TRIGGER reject_final BEFORE UPDATE OF final_head_sha ON tasks
        BEGIN SELECT RAISE(ABORT, 'capture failed'); END;",
        )
        .expect("cleanup failure");
    assert_eq!(stop_json(&store, task.id.as_str(), true).expect("JSON stop"), 5);
    assert_ne!(
        store
            .execution_snapshot(task.id.as_str())
            .expect("snapshot")
            .expect("row")["worker"]["state"],
        "settled"
    );
    assert!(
        background::load_spec_if_exists(task.id.as_str())
            .expect("barrier kept")
            .is_some()
    );
    agent.child_mut().wait().expect("owner wait");
}

#[test]
fn default_text_stop_terminal_live_worker_is_untouched_as_on_main() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, task, mut agent) = agent_fixture();
    store
        .db()
        .execute(
            "UPDATE tasks SET status = 'failed' WHERE id = ?1",
            [task.id.as_str()],
        )
        .expect("terminal");
    let before = store.execution_snapshot(task.id.as_str()).expect("before");
    let spec = std::fs::read(crate::support::paths::job_path(task.id.as_str())).expect("spec");
    assert!(
        stop(&store, task.id.as_str())
            .expect_err("main terminal refusal")
            .to_string()
            .starts_with("task already failed:")
    );
    assert!(agent.child_mut().try_wait().expect("owner wait").is_none());
    assert_eq!(store.execution_snapshot(task.id.as_str()).expect("after"), before);
    assert_eq!(
        std::fs::read(crate::support::paths::job_path(task.id.as_str())).expect("spec unchanged"),
        spec
    );
}

#[test]
fn caller_owned_stop_json_keeps_existing_derived_settlement() {
    let (_home, store) = TestHome::with_dirs().shared_store();
    let task = Task::test_fixture(TaskId("t-stop-existing".into()), TaskStatus::Done);
    store.insert_task(&task).expect("task");
    store
        .mark_worker_spawned(task.id.as_str())
        .expect("worker was spawned");
    let first = crate::cmd::execution_receipt::receipt(&store, task.id.as_str()).expect("completed");
    assert_eq!(stop_json(&store, task.id.as_str(), true).expect("existing"), 0);
    assert_eq!(
        store
            .execution_snapshot(task.id.as_str())
            .expect("snapshot")
            .expect("row")["worker"],
        first["worker"]
    );
}

#[test]
fn caller_owned_stop_json_signals_agent_group_behind_terminal_row() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, task, mut agent) = agent_fixture();
    store
        .db()
        .execute(
            "UPDATE tasks SET status = 'failed' WHERE id = ?1",
            [task.id.as_str()],
        )
        .expect("terminal");
    assert_eq!(stop_json(&store, task.id.as_str(), true).expect("stop"), 0);
    assert!(!agent.child_mut().wait().expect("only owner reaps").success());
    assert_eq!(
        store
            .execution_snapshot(task.id.as_str())
            .expect("snapshot")
            .expect("row")["worker"]["outcome"],
        "stopped"
    );
}

#[test]
fn caller_owned_text_stop_extends_terminal_row_signalling() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, task, mut agent) = agent_fixture();
    store
        .db()
        .execute(
            "UPDATE tasks SET status = 'failed', execution_policy = ?2 WHERE id = ?1",
            rusqlite::params![
                task.id.as_str(),
                serde_json::json!({"name":"caller-owned","version":1,
            "agent":"codex","model":"test","accepted_at":"now"})
                .to_string()
            ],
        )
        .expect("caller-owned");
    stop(&store, task.id.as_str()).expect("terminal worker stop");
    assert!(!agent.child_mut().wait().expect("owner wait").success());
    assert_eq!(
        crate::cmd::execution_receipt::receipt(&store, task.id.as_str()).expect("receipt")["settled"],
        true
    );
}

#[test]
fn caller_owned_agent_identity_is_persisted_at_spawn_and_survives_spec_removal() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, task, agent) = agent_fixture();
    let (recorded, _, _) = store.execution_process(task.id.as_str()).expect("evidence");
    let recorded = recorded.expect("agent");
    assert_eq!(recorded.pid, Some(agent.id()));
    assert_eq!(recorded.pgid, Some(agent.id()));
    assert!(recorded.started.is_some());
    background::clear_spec(task.id.as_str()).expect("spec removed");
    let (persisted, _, _) = store.execution_process(task.id.as_str()).expect("row evidence");
    assert_eq!(persisted.expect("agent").started, recorded.started);
    assert_eq!(
        crate::cmd::execution_receipt::receipt(&store, task.id.as_str()).expect("receipt")["settled"],
        false
    );
}

#[test]
fn caller_owned_stop_does_not_discard_a_spawned_worker_with_no_published_pid() {
    let (_home, store) = TestHome::with_dirs().shared_store();
    let task = Task::test_fixture(TaskId("t-unpublished-worker".into()), TaskStatus::Running);
    store.insert_task(&task).expect("task");
    store
        .mark_worker_spawned(task.id.as_str())
        .expect("worker spawned");
    background::save_spec(&background::BackgroundRunSpec {
        task_id: task.id.to_string(),
        ..Default::default()
    })
    .expect("unpublished worker barrier");
    assert_eq!(stop_json(&store, task.id.as_str(), true).expect("stop"), 5);
    assert!(
        background::load_spec_if_exists(task.id.as_str())
            .expect("barrier")
            .is_some()
    );
    assert_eq!(
        store
            .execution_snapshot(task.id.as_str())
            .expect("snapshot")
            .expect("row")["worker"]["state"],
        "unconfirmed"
    );
}
