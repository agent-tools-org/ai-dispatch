// Launch preparation and failure regressions for background dispatch.
// Uses two SQLite connections, bounded channel barriers and isolated job specs.

use super::*;
use crate::support::paths;
use crate::types::{AgentKind, TaskStatus};
use std::cell::RefCell;
use std::sync::mpsc;
use std::time::{Duration, Instant};

thread_local! {
    static SPAWN: RefCell<Option<Box<dyn FnOnce() -> Result<u32>>>> = const { RefCell::new(None) };
}

pub(super) fn spawn_worker() -> Result<u32> {
    SPAWN
        .with(|slot| slot.borrow_mut().take())
        .map_or_else(|| Ok(std::process::id()), |spawn| spawn())
}

fn setup() -> (
    tempfile::TempDir,
    paths::AidHomeGuard,
    Arc<Store>,
    PreparedDispatch,
    BackgroundRunSpec,
) {
    let home = tempfile::tempdir().expect("home");
    let guard = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open(&home.path().join("launch.db")).expect("store"));
    store
        .db()
        .execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at, agent_process)
         VALUES ('t-launch', 'codex', 'launch', 'running', '2026-10-23T00:00:00Z',
                 json_object('state','not_started'))",
            [],
        )
        .expect("task");
    let task = store.get_task("t-launch").expect("get").expect("task");
    let prepared = PreparedDispatch {
        detected_project: None,
        project_root: None,
        agent_kind: AgentKind::Codex,
        agent_display_name: "codex".into(),
        requested_skills: Vec::new(),
        effective_model: None,
        budget_active: false,
        task_id: task.id.clone(),
        task,
        workgroup: None,
        wt_path: None,
        effective_dir: None,
    };
    let mut spec: BackgroundRunSpec =
        serde_json::from_str(include_str!("../../../testdata/legacy-background-spec.json")).expect("spec");
    spec.task_id = "t-launch".into();
    spec.worker_pid = None;
    spec.agent_pid = None;
    spec.agent_started = None;
    (home, guard, store, prepared, spec)
}

#[test]
fn slow_spec_preparation_does_not_block_unrelated_writer() {
    let (home, _guard, store, prepared, spec) = setup();
    let writer = Store::open(&home.path().join("launch.db")).expect("writer");
    writer
        .db()
        .execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at)
         VALUES ('t-unrelated', 'codex', '', 'running', '2026-10-23T00:00:00Z')",
            [],
        )
        .expect("unrelated task");
    let (ready, entered) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let (written, observed) = mpsc::channel();
    std::thread::scope(|scope| {
        let home = home.path();
        let launch = scope.spawn(move || {
            let _guard = paths::AidHomeGuard::set(home);
            SPAWN.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(|| anyhow::bail!("launch should be refused")));
            });
            BackgroundRunSpec::on_next_save(move || {
                ready.send(()).expect("ready");
                resume.recv_timeout(Duration::from_secs(5)).expect("resume");
            });
            launch_background_worker(&store, &prepared, &spec, false)
        });
        entered
            .recv_timeout(Duration::from_secs(5))
            .expect("preparation entered");
        let write = scope.spawn(move || {
            let start = Instant::now();
            let result = writer.update_task_status("t-unrelated", TaskStatus::Stopped);
            writer
                .update_task_status("t-launch", TaskStatus::Stopped)
                .expect("stop before launch");
            written.send((result, start.elapsed())).expect("write result");
        });
        let result = observed.recv_timeout(Duration::from_secs(1));
        release.send(()).expect("release preparation");
        assert!(launch.join().expect("launch thread").is_err());
        write.join().expect("writer thread");
        let (result, elapsed) = result.expect("writer must finish while preparation is blocked");
        assert!(result.expect("write"));
        assert!(elapsed < Duration::from_secs(1));
    });
    assert!(!paths::job_path("t-launch").exists());
}

#[test]
fn stopped_task_refuses_prepared_worker_and_removes_spec() {
    let (_home, _guard, store, prepared, spec) = setup();
    store
        .update_task_status("t-launch", TaskStatus::Stopped)
        .expect("stop");
    SPAWN.with(|slot| *slot.borrow_mut() = Some(Box::new(|| panic!("must not spawn"))));
    assert!(launch_background_worker(&store, &prepared, &spec, false).is_err());
    SPAWN.with(|slot| slot.borrow_mut().take());
    assert!(!paths::job_path("t-launch").exists());
    assert_eq!(
        store.get_task("t-launch").unwrap().unwrap().status,
        TaskStatus::Stopped
    );
}

#[test]
fn spawn_failure_marks_failed_and_removes_prepared_spec() {
    let (_home, _guard, store, prepared, spec) = setup();
    SPAWN.with(|slot| *slot.borrow_mut() = Some(Box::new(|| anyhow::bail!("spawn failed"))));
    assert!(launch_background_worker(&store, &prepared, &spec, false).is_err());
    assert!(!paths::job_path("t-launch").exists());
    assert_eq!(
        store.get_task("t-launch").unwrap().unwrap().status,
        TaskStatus::Failed
    );
    crate::cmd::execution_receipt::receipt(&store, "t-launch").expect("observe launch failure");
    assert_eq!(store.execution_facts("t-launch").expect("facts").expect("row")
        .settlement.expect("settled").outcome.as_deref(), Some("launch_failed"));
}

#[test]
#[cfg(unix)]
fn pid_persistence_failure_kills_child_and_marks_failed() {
    use std::os::unix::process::CommandExt;
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, _guard, store, prepared, spec) = setup();
    let mut child = std::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .process_group(0)
        .spawn()
        .expect("child");
    let pid = child.id();
    SPAWN.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || {
            background::clear_spec("t-launch")?;
            Ok(pid)
        }))
    });
    assert!(launch_background_worker(&store, &prepared, &spec, false).is_err());
    assert!(!child.wait().expect("reap killed child").success());
    assert!(!background::is_process_running(pid));
    assert!(!paths::job_path("t-launch").exists());
    assert_eq!(
        store.get_task("t-launch").unwrap().unwrap().status,
        TaskStatus::Failed
    );
}

#[cfg(unix)]
#[path = "dispatch_execute_ordering_tests.rs"]
mod ordering;

#[path = "dispatch_execute_settlement_tests.rs"]
mod settlement;
