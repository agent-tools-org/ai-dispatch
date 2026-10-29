// Each closing path must confirm agent death, including an orphaned live group.
// Completion hooks launch before the worker's final settlement write.
use super::*;
use crate::support::test_env::TestHome;
use crate::types::{Task, TaskStatus};

fn fixture(id: &str) -> (TestHome, Arc<Store>, Task, BackgroundRunSpec) {
    let (home, store) = TestHome::with_dirs().shared_store();
    let task = Task::test_fixture(TaskId(id.into()), TaskStatus::Running);
    store.insert_task(&task).expect("task");
    let spec = BackgroundRunSpec {
        task_id: id.into(),
        ..Default::default()
    };
    save_spec(&spec).expect("spec");
    (home, store, task, spec)
}

#[cfg(unix)]
fn live_agent(spec: &mut BackgroundRunSpec) -> crate::runtime::process::guard::ProcessGuard {
    let mut command = std::process::Command::new("sleep");
    command.arg("60");
    let child = crate::runtime::process::guard::ProcessGuard::spawn(&mut command).expect("agent");
    spec.agent_pid = Some(child.id());
    spec.agent_started = crate::runtime::process::monitor::activity::root_started(child.id());
    assert!(spec.agent_started.is_some());
    save_spec(spec).expect("spec");
    child
}

fn assert_open(store: &Store, id: &str) {
    assert!(
        !store
            .execution_facts(id)
            .expect("facts")
            .expect("row")
            .settlement
            .is_some_and(|s| s.state == "settled")
    );
}

#[cfg(unix)]
#[test]
fn caller_owned_worker_cannot_close_while_recorded_agent_lives() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, task, mut spec) = fixture("t-worker-live-agent");
    let mut agent = live_agent(&mut spec);
    finish_worker(&store, &spec, Ok(())).expect("finish cleanup");
    assert_open(&store, task.id.as_str());
    assert!(agent.child_mut().try_wait().expect("owner wait").is_none());
    assert!(load_spec_if_exists(task.id.as_str()).expect("barrier").is_some());
}

#[cfg(unix)]
#[test]
fn caller_owned_observation_never_settles_dead_worker_with_live_agent() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, task, mut spec) = fixture("t-observe-live-agent");
    spec.worker_pid = Some(u32::MAX);
    let mut agent = live_agent(&mut spec);
    let receipt = crate::cmd::execution_receipt::receipt(&store, task.id.as_str()).expect("receipt");
    assert_eq!(receipt["settled"], false);
    assert_eq!(receipt["worker"]["state"], "unconfirmed");
    assert!(agent.child_mut().try_wait().expect("owner wait").is_none());
}

#[cfg(unix)]
#[test]
fn caller_owned_reaper_closes_only_after_agent_exit_and_cleanup() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store, task, mut spec) = fixture("t-reaper-live-agent");
    spec.worker_pid = Some(u32::MAX);
    let mut agent = live_agent(&mut spec);
    let mut cleaned = Vec::new();
    // The reaper's signal seam records kills; the live agent intentionally survives.
    reaper::cleanup_pid_task(&store, &task, &spec, u32::MAX, &mut cleaned, &|_| false, 60).expect("cleanup");
    assert_eq!(
        store
            .get_task(task.id.as_str())
            .expect("task")
            .expect("row")
            .status,
        TaskStatus::Failed
    );
    assert_open(&store, task.id.as_str());
    assert!(agent.child_mut().try_wait().expect("owner wait").is_none());
    assert!(
        load_spec_if_exists(task.id.as_str())
            .expect("evidence retained")
            .is_some()
    );
}

#[test]
fn caller_owned_completion_hook_launch_precedes_worker_settlement() {
    let (_home, store, task, mut spec) = fixture("t-hook-before-close");
    spec.on_done = Some("true".into());
    save_spec(&spec).expect("spec");
    let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let seen = observed.clone();
    let hook_store = store.clone();
    let id = task.id.to_string();
    process::ON_DONE_SPAWN.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || {
            assert_open(&hook_store, &id);
            assert!(load_spec_if_exists(&id).expect("spec removed").is_none());
            seen.store(true, std::sync::atomic::Ordering::SeqCst);
        }))
    });
    finish_worker(&store, &spec, Ok(())).expect("finish");
    assert!(
        observed.load(std::sync::atomic::Ordering::SeqCst),
        "hook was launched"
    );
    assert_eq!(
        store
            .execution_facts(task.id.as_str())
            .expect("facts")
            .expect("row")
            .settlement
            .expect("settled")
            .outcome
            .as_deref(),
        Some("completed")
    );
}
