// Observation races, read-only receipts and orphan reaping regressions.
// Uses existing group fixtures without adding process ownership or settlement state.
use super::*;
use super::barrier_tests::{fixture, live_agent, record_agent};
use crate::types::TaskStatus;

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
fn caller_owned_show_receipt_keeps_persisted_state_without_any_probe_or_write() {
    let (_home, store, id) = fixture();
    store.mark_worker_spawned(id.as_str()).expect("spawn");
    let before = store.execution_snapshot(id.as_str()).expect("snapshot");
    let changes = store.db().total_changes();
    std::fs::write(crate::support::paths::job_path(id.as_str()), "unreadable job spec").expect("spec");
    let shown = read_only_receipt(&store, id.as_str()).expect("show does not inspect the spec");
    assert_eq!(shown["settled"], false);
    assert!(shown["worker"]["outcome"].is_null());
    assert_eq!(store.execution_snapshot(id.as_str()).expect("snapshot"), before);
    assert_eq!(store.db().total_changes(), changes);
}

#[test]
fn caller_owned_show_receipt_does_not_observe_a_running_worker() {
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
        "pending"
    );
    assert_eq!(store.execution_snapshot(id.as_str()).expect("snapshot"), before);
}

#[test]
fn caller_owned_reaper_marks_a_crashed_worker_failed_like_default_mode() {
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
            .contains(&id.to_string())
    );
    assert_eq!(store.get_task(id.as_str()).expect("task").expect("row").status, TaskStatus::Failed);
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
