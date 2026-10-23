// Agent cleanup when stop kills the worker before its former PID publication point.
// Real piped/PTY runners use socket barriers across separate worker and agent groups.

use super::*;
use crate::runtime::process::guard::ProcessGuard;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

const HELPER: &str = "store::status_mutations::tests::worker_death::worker_helper";
const AGENT: &str = "store::status_mutations::tests::worker_death::agent_helper";

struct AgentCleanup(u32);

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

fn worker(home: &Path, mode: &str) -> ProcessGuard {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "shift; \"$@\" & wait \"$!\"", "__run-task", "t-launch"])
        .arg(std::env::current_exe().expect("test binary"))
        .args(["--exact", HELPER, "--ignored", "--nocapture"])
        .env("AID_LAUNCH_TEST_HOME", home)
        .env("AID_LAUNCH_TEST_MODE", mode)
        .stdout(Stdio::null());
    ProcessGuard::spawn(&mut cmd).expect("worker")
}

pub(super) fn assert_stop_kills_worker_and_agent() {
    let _permit = crate::support::test_subprocess::acquire();
    for mode in ["piped", "pty"] {
        let (home, _guard, _launch, stop) = setup();
        let agents = UnixListener::bind(home.path().join("agent.sock")).expect("agent barrier");
        let workers = UnixListener::bind(home.path().join("worker.sock")).expect("worker barrier");
        let mut worker = worker(home.path(), mode);
        let mut agent = accept(&agents);
        let pid = line(&mut agent).parse::<u32>().expect("agent PID");
        let _cleanup = AgentCleanup(pid);
        let mut committed = accept(&workers);
        assert_eq!(line(&mut committed), "committed");
        let spec = background::load_spec_if_exists("t-launch").unwrap().unwrap();
        assert_eq!(spec.worker_pid, Some(worker.id()));
        crate::cmd::stop::kill(&stop, "t-launch").expect("stop kills worker first");
        assert!(!worker.wait().expect("reap worker").success());
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
    background::update_worker_pid("t-launch", unsafe { libc::getppid() } as u32).expect("worker PID");
    pause_worker_after_commit();
    let agent = crate::agent::get_agent(AgentKind::Grok);
    let id = TaskId("t-launch".into());
    if std::env::var("AID_LAUNCH_TEST_MODE").expect("mode") == "pty" {
        run_pty(agent.as_ref(), &id, &store);
    } else {
        run_piped(agent.as_ref(), &id, &store).await;
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
