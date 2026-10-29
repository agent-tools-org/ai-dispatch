// Observation races, read-only receipts and orphan reaping regressions.
// Uses existing group fixtures without adding process ownership or settlement state.
use super::*;
use super::barrier_tests::{fixture, live_agent, record_agent};

#[test]
fn caller_owned_normal_completion_race_rechecks_the_spec_before_labeling_lost() {
    let (_home, store, id) = fixture();
    store.mark_worker_spawned(id.as_str()).expect("spawn");
    background::save_spec(
        &serde_json::from_value(serde_json::json!({"task_id":id.as_str(),"worker_pid":4294967295u32}))
            .expect("spec"),
    )
    .expect("spec");
    let removed = id.to_string();
    OBSERVE_HOOK.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || {
            background::clear_spec(&removed).expect("normal completion removes spec");
        }))
    });
    assert_eq!(
        receipt(&store, id.as_str()).expect("observe")["worker"]["outcome"],
        "completed"
    );
}

#[test]
fn caller_owned_read_only_receipt_derives_without_any_store_or_spec_write() {
    let (_home, store, id) = fixture();
    store.mark_worker_spawned(id.as_str()).expect("spawn");
    let before = store.execution_snapshot(id.as_str()).expect("snapshot");
    let changes = store.db().total_changes();
    let shown = read_only_receipt(&store, id.as_str()).expect("show");
    assert_eq!(shown["settled"], true);
    assert_eq!(shown["worker"]["outcome"], "completed");
    assert_eq!(store.execution_snapshot(id.as_str()).expect("snapshot"), before);
    assert_eq!(store.db().total_changes(), changes);
}

#[test]
fn caller_owned_read_only_receipt_reports_running_without_publishing_worker_state() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, id) = fixture();
    let mut command = std::process::Command::new("sh");
    command.args(["-c", "sleep 60", "__run-task", id.as_str()]);
    let _worker = crate::runtime::process::guard::ProcessGuard::spawn(&mut command).expect("worker");
    background::save_spec(
        &serde_json::from_value(serde_json::json!({"task_id":id.as_str(),"worker_pid":_worker.id()}))
            .expect("spec"),
    )
    .expect("spec");
    let before = store.execution_snapshot(id.as_str()).expect("snapshot");
    assert_eq!(
        read_only_receipt(&store, id.as_str()).expect("read only")["worker"]["state"],
        "running"
    );
    assert_eq!(store.execution_snapshot(id.as_str()).expect("snapshot"), before);
}

#[test]
fn caller_owned_reaper_observation_preserves_the_live_orphan_agent_and_its_group_identity() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, id) = fixture();
    let mut agent = live_agent();
    record_agent(&store, &id, &agent);
    store.db().execute("UPDATE tasks SET execution_policy=?2 WHERE id=?1",rusqlite::params![id.as_str(),
        serde_json::json!({"name":"caller-owned","version":1,"agent":"codex","model":"test","accepted_at":"now"}).to_string()]).expect("policy");
    background::save_spec(&serde_json::from_value(serde_json::json!({"task_id":id.as_str(),"worker_pid":4294967295u32,
        "agent_pid":agent.id(),"agent_started":crate::runtime::process::monitor::activity::root_started(agent.id())})).expect("spec")).expect("spec");
    assert!(
        background::check_zombie_tasks_with(&store, |_| false)
            .expect("startup reaper")
            .is_empty()
    );
    let running = receipt(&store, id.as_str()).expect("observe");
    assert_eq!(running["settled"], false);
    assert_eq!(running["agent_process"]["pid"], agent.id());
    assert_eq!(running["agent_process"]["pgid"], agent.id());
    assert!(agent.child_mut().try_wait().expect("owner wait").is_none());
}

#[test]
fn caller_owned_running_receipt_distinguishes_worker_pid_from_persisted_agent_pid_and_group() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, id) = fixture();
    let mut agent = live_agent();
    record_agent(&store, &id, &agent);
    let mut command = std::process::Command::new("sh");
    command.args(["-c", "sleep 60", "__run-task", id.as_str()]);
    let worker = crate::runtime::process::guard::ProcessGuard::spawn(&mut command).expect("worker");
    background::save_spec(&serde_json::from_value(serde_json::json!({"task_id":id.as_str(),"worker_pid":worker.id(),
        "agent_pid":agent.id(),"agent_started":crate::runtime::process::monitor::activity::root_started(agent.id())})).expect("spec")).expect("spec");
    let observed = receipt(&store, id.as_str()).expect("receipt");
    assert_eq!(observed["worker"]["pid"], worker.id());
    assert_eq!(observed["agent_process"]["pid"], agent.id());
    assert_eq!(observed["agent_process"]["pgid"], agent.id());
    assert_ne!(observed["worker"]["pid"], observed["agent_process"]["pid"]);
    assert!(agent.child_mut().try_wait().expect("owner wait").is_none());
}
