// Agent cleanup when stop kills the worker before its former PID publication point.
// Real piped/PTY runners use socket barriers across separate worker and agent groups.

use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::Path;

const HELPER: &str = "store::status_mutations::tests::worker_death::worker_helper";
const AGENT: &str = "store::status_mutations::tests::worker_death::agent_helper";

struct AgentCleanup(u32);

struct Worker {
    child: Child,
    stopped: bool,
}

impl Drop for Worker {
    fn drop(&mut self) {
        if !self.stopped {
            background::sigkill_process(self.child.id());
            let _ = self.child.wait();
        }
    }
}

impl Drop for AgentCleanup {
    fn drop(&mut self) {
        background::sigkill_process(self.0);
    }
}

fn accept(listener: &UnixListener) -> UnixStream {
    let mut fd = libc::pollfd {
        fd: listener.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    assert!(
        unsafe { libc::poll(&mut fd, 1, 5_000) } > 0,
        "helper must reach barrier"
    );
    let stream = listener.accept().expect("accept barrier").0;
    stream.set_read_timeout(Some(DEADLINE)).expect("barrier deadline");
    stream
}

fn line(stream: &mut UnixStream) -> String {
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).expect("barrier line");
    line.trim().to_string()
}

fn worker(home: &Path, mode: &str, store: &Store) -> Worker {
    if mode == "codex" {
        prepare_codex(home);
    }
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "shift; \"$@\" & wait \"$!\"", "__run-task", "t-launch"])
        .arg(std::env::current_exe().expect("test binary"))
        .args(["--exact", HELPER, "--ignored", "--nocapture"])
        .env("AID_LAUNCH_TEST_HOME", home)
        .env("AID_LAUNCH_TEST_MODE", mode)
        .stdout(Stdio::null())
        .process_group(0);
    store
        .with_worker_launch("t-launch", || {
            let worker = Worker {
                child: cmd.spawn()?,
                stopped: false,
            };
            background::update_worker_pid("t-launch", worker.child.id())?;
            Ok(worker)
        })
        .expect("worker launch")
}

pub(super) fn assert_stop_kills_worker_and_agent() {
    let _permit = crate::support::test_subprocess::acquire();
    for mode in ["piped", "pty", "codex"] {
        let (home, _guard, launch, stop) = setup();
        let agents = UnixListener::bind(home.path().join("agent.sock")).expect("agent barrier");
        let workers = UnixListener::bind(home.path().join("worker.sock")).expect("worker barrier");
        let mut worker = worker(home.path(), mode, &launch);
        let mut agent = accept(&agents);
        let pid = line(&mut agent).parse::<u32>().expect("agent PID");
        let _cleanup = AgentCleanup(pid);
        let mut committed = accept(&workers);
        assert_eq!(line(&mut committed), "committed");
        let spec = background::load_spec_if_exists("t-launch").unwrap().unwrap();
        assert_eq!(spec.worker_pid, Some(worker.child.id()));
        crate::cmd::stop::kill(&stop, "t-launch").expect("stop kills worker first");
        worker.stopped = true;
        assert!(
            !crate::support::test_subprocess::is_live(worker.child.id() as i32),
            "worker survived stop"
        );
        assert_eq!(committed.read(&mut [0]).expect("worker barrier closed"), 0);
        assert_eq!(agent.read(&mut [0]).expect("agent barrier closed"), 0);
        assert_eq!(
            spec.agent_pid,
            Some(pid),
            "PID must be visible before worker can die"
        );
        assert!(
            !crate::support::test_subprocess::is_live(pid as i32),
            "agent survived worker"
        );
        assert_eq!(
            stop.get_task("t-launch").unwrap().unwrap().status,
            TaskStatus::Stopped
        );
        assert!(!paths::job_path("t-launch").exists());
    }
}

fn home() -> std::path::PathBuf {
    std::env::var_os("AID_LAUNCH_TEST_HOME")
        .expect("isolated home")
        .into()
}

fn command() -> Command {
    let mut cmd = Command::new(std::env::current_exe().expect("test binary"));
    cmd.args(["--exact", AGENT, "--ignored", "--nocapture"]);
    cmd.env("AID_LAUNCH_TEST_HOME", home());
    cmd
}

fn pause_worker_after_commit() {
    AFTER_COMMIT.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(|| {
            let mut barrier = UnixStream::connect(home().join("worker.sock")).expect("worker barrier");
            writeln!(barrier, "committed").expect("commit signal");
            barrier
                .set_read_timeout(Some(Duration::from_secs(30)))
                .expect("worker deadline");
            let _ = barrier.read(&mut [0]);
            panic!("worker must be killed without resuming publication");
        }));
    });
}

#[tokio::test]
#[ignore]
async fn worker_helper() {
    let home = home();
    let _guard = paths::AidHomeGuard::set(&home);
    let store = Arc::new(Store::open(&home.join("launch.db")).expect("worker store"));
    pause_worker_after_commit();
    let agent = crate::agent::get_agent(AgentKind::Grok);
    let id = TaskId("t-launch".into());
    match std::env::var("AID_LAUNCH_TEST_MODE").expect("mode").as_str() {
        "pty" => run_pty(agent.as_ref(), &id, &store),
        "codex" => run_codex(&id, &store),
        _ => run_piped(agent.as_ref(), &id, &store).await,
    }
    panic!("worker must not reach monitoring");
}

fn run_pty(agent: &dyn crate::agent::Agent, id: &TaskId, store: &Arc<Store>) {
    crate::runtime::pty::runner::run_agent_process_with_control(
        agent,
        &command(),
        id,
        store,
        &paths::log_path(id.as_str()),
        None,
        None,
        false,
        crate::config::timeout_policy::TimeoutPolicy::default(),
        None,
        None,
        std::time::Instant::now(),
    )
    .expect("PTY launch");
}

async fn run_piped(agent: &dyn crate::agent::Agent, id: &TaskId, store: &Arc<Store>) {
    crate::cmd::run::run_agent_process_with_cost(
        agent,
        tokio::process::Command::from(command()),
        id,
        store,
        &paths::log_path(id.as_str()),
        None,
        None,
        false,
        None,
        crate::config::timeout_policy::TimeoutPolicy::default(),
        None,
        std::time::Instant::now(),
    )
    .await
    .expect("piped launch");
}

#[test]
#[ignore]
fn agent_helper() {
    let mut barrier = UnixStream::connect(home().join("agent.sock")).expect("agent barrier");
    writeln!(barrier, "{}", std::process::id()).expect("agent PID");
    barrier
        .set_read_timeout(Some(Duration::from_secs(30)))
        .expect("agent deadline");
    let _ = barrier.read(&mut [0]);
    panic!("agent must be signalled by stop");
}

fn prepare_codex(home: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let path = home.join("codex");
    std::fs::write(
        &path,
        concat!(
            "#!/bin/sh\n",
            "[ \"$1\" = app-server ] || exit 1\n",
            "exec \"$AID_TEST_BINARY\" --exact \"$AID_TEST_AGENT\" --ignored --nocapture\n",
        ),
    )
    .expect("fake Codex executable");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).expect("executable");
}

fn run_codex(id: &TaskId, store: &Arc<Store>) {
    let mut command = Command::new(home().join("codex"));
    command
        .args(["exec", "synthetic prompt"])
        .env("AID_TEST_BINARY", std::env::current_exe().expect("test binary"))
        .env("AID_TEST_AGENT", AGENT)
        .env("AID_LAUNCH_TEST_HOME", home());
    let agent = crate::agent::get_agent(AgentKind::Codex);
    crate::runtime::codex_app_server::run(crate::runtime::codex_app_server::Run {
        start: std::time::Instant::now(),
        agent: agent.as_ref(),
        command: &command,
        task: id,
        store,
        log: &paths::log_path(id.as_str()),
        output: None,
        group: None,
        policy: crate::config::timeout_policy::TimeoutPolicy::default(),
        max_cost: None,
        control: None,
    })
    .expect("Codex app-server launch");
}

mod ordering {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        static REPORTED: Cell<bool> = const { Cell::new(false) };
    }

    fn blocked_publication(_: i32) -> bool {
        if !REPORTED.with(|reported| reported.replace(true)) {
            let mut barrier = UnixStream::connect(home().join("ordering.sock")).expect("ordering barrier");
            barrier
                .set_read_timeout(Some(DEADLINE))
                .expect("ordering deadline");
            writeln!(barrier, "blocked").expect("publication blocked");
            assert_eq!(barrier.read(&mut [0]).expect("retry publication"), 1);
        }
        true
    }

    #[tokio::test]
    #[ignore]
    async fn worker_helper() {
        assert_eq!(std::env::var("AID_NO_DETACH").expect("non-detached mode"), "1");
        let home = home();
        let _guard = paths::AidHomeGuard::set(&home);
        let mut start = UnixStream::connect(home.join("start.sock")).expect("start barrier");
        start.set_read_timeout(Some(DEADLINE)).expect("start deadline");
        assert_eq!(start.read(&mut [0]).expect("start agent launch"), 1);
        let conn = rusqlite::Connection::open(home.join("launch.db")).expect("existing database");
        conn.busy_handler(Some(blocked_publication))
            .expect("writer barrier");
        let store = Arc::new(Store {
            conn: std::sync::Mutex::new(conn),
        });
        pause_worker_after_commit();
        let agent = crate::agent::get_agent(AgentKind::Grok);
        run_piped(agent.as_ref(), &TaskId("t-launch".into()), &store).await;
        panic!("stop must kill the worker before monitoring");
    }
}
