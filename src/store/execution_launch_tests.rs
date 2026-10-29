// Spawn evidence recovery preserves main's launch behavior and one owned launch.
// Fixtures use isolated stores and owned children; publication errors never reap children.
use super::*;
use crate::{types::*, support::test_env::TestHome, runtime::background};

fn fixture(policy: bool) -> (TestHome, std::sync::Arc<Store>, TaskId) {
    let (home, store) = TestHome::with_dirs().shared_store();
    let id = TaskId("t-evidence-launch".into());
    store
        .insert_task(&Task::test_fixture(id.clone(), TaskStatus::Running))
        .expect("task");
    if policy {
        store
            .db()
            .execute(
                "UPDATE tasks SET execution_policy = ?2 WHERE id = ?1",
                params![
                    id.as_str(),
                    serde_json::json!({"name":"caller-owned","version":1,"agent":"codex",
            "model":"test","accepted_at":"now"})
                    .to_string()
                ],
            )
            .expect("policy");
    }
    (home, store, id)
}

#[test]
fn caller_owned_guard_refusal_restores_not_started_evidence() {
    let (_home, store, id) = fixture(true);
    store
        .update_task_status(id.as_str(), TaskStatus::Stopped)
        .expect("stopped");
    assert!(
        store
            .with_running_task(true, id.as_str(), || -> Result<()> {
                panic!("guard must refuse");
            })
            .is_err()
    );
    assert_eq!(
        store
            .execution_process(id.as_str())
            .expect("row")
            .0
            .expect("agent")
            .state,
        "not_started"
    );
    assert_eq!(
        crate::cmd::stop::stop_json(&store, id.as_str(), true).expect("stop"),
        0
    );
}

#[cfg(unix)]
#[test]
fn default_launch_skips_evidence_writes_even_when_publication_would_fail() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, id) = fixture(false);
    store
        .db()
        .execute_batch(
            "CREATE TRIGGER reject_agent BEFORE UPDATE OF agent_process ON tasks
        WHEN json_extract(NEW.agent_process,'$.state') = 'spawned'
        BEGIN SELECT RAISE(ABORT,'publication unavailable'); END;",
        )
        .expect("failure");
    let mut child = store
        .with_running_task(false, id.as_str(), || {
            let mut command = std::process::Command::new("sleep");
            command.arg("60");
            let child = crate::runtime::process::guard::ProcessGuard::spawn(&mut command)?;
            background::update_agent_pid(id.as_str(), child.id())?;
            Ok(child)
        })
        .expect("live launch must succeed");
    assert!(child.child_mut().try_wait().expect("owner wait").is_none());
    assert_eq!(
        store
            .execution_process(id.as_str())
            .expect("row")
            .0
            .expect("agent")
            .state,
        "not_started"
    );
}

#[test]
fn caller_owned_group_id_is_the_launch_pid_even_if_the_child_has_exited() {
    let (_home, store, id) = fixture(true);
    let spec: background::BackgroundRunSpec = serde_json::from_value(serde_json::json!({
        "task_id":id.as_str(),"agent_pid":2147483647,"agent_started":"launch"}))
    .expect("spec");
    background::save_spec(&spec).expect("spec");
    record_agent_on(&store.db(), id.as_str()).expect("publish");
    assert_eq!(
        store
            .execution_process(id.as_str())
            .expect("row")
            .0
            .expect("agent")
            .pgid,
        Some(2147483647)
    );
}

#[cfg(unix)]
#[test]
fn caller_owned_second_agent_launch_is_refused_without_overwriting_the_first_group() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, id) = fixture(true);
    let _child = store
        .with_running_task(true, id.as_str(), || {
            let mut command = std::process::Command::new("sleep");
            command.arg("60");
            let child = crate::runtime::process::guard::ProcessGuard::spawn(&mut command)?;
            background::update_agent_pid(id.as_str(), child.id())?;
            Ok(child)
        })
        .expect("first launch");
    let before = store
        .db()
        .query_row(
            "SELECT agent_process FROM tasks WHERE id=?1",
            [id.as_str()],
            |r| r.get::<_, String>(0),
        )
        .expect("row");
    assert!(
        store
            .with_running_task(true, id.as_str(), || -> Result<()> {
                panic!("second launch");
            })
            .is_err()
    );
    assert_eq!(
        store
            .db()
            .query_row(
                "SELECT agent_process FROM tasks WHERE id=?1",
                [id.as_str()],
                |r| r.get::<_, String>(0)
            )
            .expect("row"),
        before
    );
}

#[cfg(unix)]
#[test]
fn caller_owned_confirmed_publication_kill_recovers_evidence_and_keeps_launch_consumed() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, id) = fixture(true);
    background::BackgroundRunSpec::prepare_agent(id.as_str()).expect("spec");
    let path = crate::support::paths::job_path(id.as_str());
    let blocked = path.clone();
    background::BackgroundRunSpec::on_next_save(move || {
        std::fs::remove_file(&blocked).expect("remove");
        std::fs::create_dir(&blocked).expect("block publication");
    });
    let mut owner = None;
    let result = store.with_running_task(true, id.as_str(), || {
        let mut command = std::process::Command::new("sleep");
        command.arg("60");
        let child = crate::runtime::process::guard::ProcessGuard::spawn(&mut command)?;
        let pid = child.id();
        owner = Some(child);
        background::publish_agent_pid(true, id.as_str(), pid)
    });
    assert!(result.is_err());
    owner
        .as_mut()
        .expect("owner")
        .child_mut()
        .wait()
        .expect("only owner reaps");
    assert_eq!(
        store
            .execution_process(id.as_str())
            .expect("row")
            .0
            .expect("agent")
            .state,
        "not_started"
    );
    std::fs::remove_dir(&path).expect("unblock");
    assert!(
        store
            .with_running_task(true, id.as_str(), || -> Result<()> {
                panic!("launch already consumed");
            })
            .is_err()
    );
    assert_eq!(
        crate::cmd::stop::stop_json(&store, id.as_str(), true).expect("confirmed dead stop"),
        0
    );
}
