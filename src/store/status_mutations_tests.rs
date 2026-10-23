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
    let (published, visible) = mpsc::channel();
    std::thread::scope(|scope| {
        let home = home.path();
        let launch_thread = scope.spawn(move || {
            let _guard = paths::AidHomeGuard::set(home);
            let child = launch
                .with_running_task("t-launch", || {
                    let child = child();
                    entered.send(()).expect("guard entered");
                    resume.recv_timeout(DEADLINE).expect("release guard");
                    Ok(child)
                })
                .expect("launch");
            launch
                .publish_launch("t-launch", child.id(), || {
                    background::update_agent_pid("t-launch", child.id())
                })
                .expect("publish");
            published.send(()).expect("PID visible");
            child
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
        visible.recv_timeout(DEADLINE).expect("published after commit");
        allow_stop.send(()).expect("resume stop");
        stop_thread.join().expect("stop thread");
        assert_stopped(&stop, launch_thread.join().expect("launch thread"));
    });
}

#[test]
#[cfg(unix)]
fn stop_before_pid_publication_kills_child_and_removes_resurrected_spec() {
    let _permit = crate::support::test_subprocess::acquire();
    let (home, _guard, launch, stop) = setup();
    let (saving, prepared) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    std::thread::scope(|scope| {
        let home = home.path();
        let handle = scope.spawn(move || {
            let _guard = paths::AidHomeGuard::set(home);
            let child = launch
                .with_running_task("t-launch", || Ok(child()))
                .expect("launch");
            BackgroundRunSpec::on_next_save(move || {
                saving.send(()).expect("PID loaded before stop");
                resume.recv_timeout(DEADLINE).expect("publish after stop");
            });
            let result = launch.publish_launch("t-launch", child.id(), || {
                background::update_agent_pid("t-launch", child.id())
            });
            (result, child)
        });
        prepared.recv_timeout(DEADLINE).expect("saving PID outside guard");
        crate::cmd::stop::stop(&stop, "t-launch").expect("stop before publication");
        release.send(()).expect("release PID publication");
        let (result, child) = handle.join().expect("launch thread");
        assert!(
            result.is_err(),
            "post-commit status check must reject stopped task"
        );
        assert_stopped(&stop, child);
    });
}
