// Launch/stop ordering at transaction commit and PID publication boundaries.
// Uses real child processes, two SQLite connections and channel barriers.

use super::*;
use crate::runtime::background::{self, BackgroundRunSpec};
use crate::support::paths;
use std::cell::RefCell;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, mpsc};
use std::time::Duration;

const DEADLINE: Duration = Duration::from_secs(5);
type Barrier = (mpsc::Sender<()>, mpsc::Receiver<()>);
thread_local! {
    static BUSY: RefCell<Option<Barrier>> = const { RefCell::new(None) };
    static AFTER_COMMIT: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

pub(super) fn agent_committed() {
    let hook = AFTER_COMMIT.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

fn blocked_stop(_: i32) -> bool {
    BUSY.with(|slot| {
        if let Some((ready, release)) = slot.borrow_mut().take() {
            ready.send(()).expect("stop blocked on launch");
            release.recv_timeout(DEADLINE).expect("release stop");
        }
    });
    true
}

fn setup() -> (tempfile::TempDir, paths::AidHomeGuard, Arc<Store>, Arc<Store>) {
    let home = tempfile::tempdir().expect("home");
    let guard = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("dirs");
    let path = home.path().join("launch.db");
    let launch = Arc::new(Store::open(&path).expect("launch store"));
    let stop = Arc::new(Store::open(&path).expect("stop store"));
    launch
        .db()
        .execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at)
         VALUES ('t-launch', 'claude', 'launch', 'running', '2026-10-23T00:00:00Z')",
            [],
        )
        .expect("task");
    let mut spec: BackgroundRunSpec =
        serde_json::from_str(include_str!("../../testdata/legacy-background-spec.json")).expect("spec");
    spec.task_id = "t-launch".into();
    spec.worker_pid = None;
    spec.agent_pid = None;
    spec.agent_started = None;
    background::save_spec(&spec).expect("spec saved");
    (home, guard, launch, stop)
}

#[cfg(unix)]
fn child() -> Child {
    use std::os::unix::process::CommandExt;
    Command::new("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .process_group(0)
        .spawn()
        .expect("child")
}

fn assert_stopped(store: &Store, mut child: Child) {
    let pid = child.id();
    assert!(!child.wait().expect("reap child").success());
    assert!(!background::is_process_running(pid));
    assert_eq!(
        store.get_task("t-launch").unwrap().unwrap().status,
        TaskStatus::Stopped
    );
    assert!(!paths::job_path("t-launch").exists());
}

#[test]
#[cfg(unix)]
fn stop_waits_for_launch_guard_then_signals_published_child() {
    let _permit = crate::support::test_subprocess::acquire();
    let (home, _guard, launch, stop) = setup();
    let (entered, guarded) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let (busy, blocked) = mpsc::channel();
    let (allow_stop, resume_stop) = mpsc::channel();
    std::thread::scope(|scope| {
        let home = home.path();
        let launch_thread = scope.spawn(move || {
            let _guard = paths::AidHomeGuard::set(home);
            launch
                .with_running_task(false, "t-launch", |_pid| {
                    let child = child();
                    entered.send(()).expect("guard entered");
                    resume.recv_timeout(DEADLINE).expect("release guard");
                    background::update_agent_pid("t-launch", child.id())?;
                    Ok(child)
                })
                .expect("launch")
        });
        guarded.recv_timeout(DEADLINE).expect("inside guard");
        let stopping = stop.clone();
        let stop_thread = scope.spawn(move || {
            let _guard = paths::AidHomeGuard::set(home);
            BUSY.with(|slot| *slot.borrow_mut() = Some((busy, resume_stop)));
            stopping
                .db()
                .busy_handler(Some(blocked_stop))
                .expect("busy callback");
            crate::cmd::stop::stop(&stopping, "t-launch").expect("stop");
        });
        blocked.recv_timeout(DEADLINE).expect("stop must wait for guard");
        release.send(()).expect("commit launch");
        allow_stop.send(()).expect("resume stop");
        stop_thread.join().expect("stop thread");
        assert_stopped(&stop, launch_thread.join().expect("launch thread"));
    });
}

#[test]
#[cfg(unix)]
fn stop_before_old_pid_publication_point_kills_worker_and_agent() {
    worker_death::assert_stop_kills_worker_and_agent();
}

#[cfg(unix)]
#[path = "status_mutations_worker_tests.rs"]
mod worker_death;

#[test]
fn parent_stopped_after_worker_guard_refuses_agent_guard() {
    let (_home, _guard, launch, stop) = setup();
    launch
        .db()
        .execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at)
         VALUES ('t-parent', 'claude', '', 'running', '2026-10-23T00:00:00Z')",
            [],
        )
        .expect("parent");
    launch
        .db()
        .execute(
            "UPDATE tasks SET parent_task_id = 't-parent' WHERE id = 't-launch'",
            [],
        )
        .expect("child parent");
    launch
        .with_worker_launch(false, "t-launch", || Ok(()))
        .expect("worker launched");
    stop.update_task_status("t-parent", TaskStatus::Stopped)
        .expect("stop parent");
    let error = launch
        .with_running_task(false, "t-launch", |_pid| -> Result<()> {
            panic!("agent must not spawn after parent stop")
        })
        .expect_err("refused agent launch");
    assert!(error.to_string().contains("t-parent"));
    assert_eq!(
        launch.get_task("t-launch").unwrap().unwrap().status,
        TaskStatus::Stopped
    );
    assert!(!paths::job_path("t-launch").exists());
}

fn stalled_handoff(
    launch: Arc<Store>,
    entered: mpsc::Sender<()>,
    resume: mpsc::Receiver<()>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut cmd = Command::new("cat");
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped());
        let (result, mut child) = launch
            .with_worker_launch(false, "t-launch", || {
                BackgroundRunSpec::test_worker_handoff(&mut cmd, || {
                    entered.send(()).expect("inside handoff guard");
                    resume.recv_timeout(DEADLINE).expect("resume PID wait");
                })
            })
            .expect("guard completed");
        child.kill().expect("kill fake launcher");
        child.wait().expect("reap after guard");
        assert!(
            result
                .expect_err("stalled launcher")
                .to_string()
                .contains("timed out")
        );
    })
}

#[test]
#[cfg(unix)]
fn unrelated_writer_waits_inside_handoff_then_finishes_well_before_busy_timeout() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, _guard, launch, writer) = setup();
    writer
        .db()
        .execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at)
         SELECT 't-unrelated', agent, prompt, status, created_at FROM tasks WHERE id = 't-launch'",
            [],
        )
        .expect("unrelated task");
    let (entered, guarded) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let (busy, blocked) = mpsc::channel();
    let (retry, retry_write) = mpsc::channel();
    let (written, observed) = mpsc::channel();
    let launch_thread = stalled_handoff(launch, entered, resume);
    guarded.recv_timeout(DEADLINE).expect("guard is active");
    let writer_thread = std::thread::spawn(move || {
        BUSY.with(|slot| *slot.borrow_mut() = Some((busy, retry_write)));
        writer
            .db()
            .busy_handler(Some(blocked_stop))
            .expect("busy callback");
        let start = std::time::Instant::now();
        let result = writer.update_task_status("t-unrelated", TaskStatus::Stopped);
        written.send((result, start.elapsed())).expect("unrelated write");
    });
    blocked
        .recv_timeout(DEADLINE)
        .expect("writer blocked inside guard");
    assert!(
        observed.try_recv().is_err(),
        "writer must wait for the atomic handoff"
    );
    release.send(()).expect("start bounded PID wait");
    retry.send(()).expect("retry unrelated writer");
    let (result, elapsed) = observed
        .recv_timeout(Duration::from_secs(2))
        .expect("prompt write");
    assert!(result.expect("write"));
    assert!(
        elapsed < Duration::from_secs(2),
        "writer exceeded handoff bound: {elapsed:?}"
    );
    launch_thread.join().expect("launch thread");
    writer_thread.join().expect("writer thread");
}
