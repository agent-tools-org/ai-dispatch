// Non-detached worker/agent PID rewrites must share launch-transaction ordering.
// Uses the dispatch spawn seam, a real worker process and socket barriers.

use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::rc::Rc;

const HELPER: &str = "store::status_mutations::tests::worker_death::ordering::worker_helper";

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

struct AgentCleanup(u32);

impl Drop for AgentCleanup {
    fn drop(&mut self) {
        background::sigkill_process(self.0);
    }
}

fn ready(listeners: &[&UnixListener]) -> Vec<libc::pollfd> {
    let mut fds: Vec<_> = listeners
        .iter()
        .map(|listener| libc::pollfd {
            fd: listener.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();
    assert!(
        unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 5_000) } > 0,
        "worker must reach a publication barrier"
    );
    fds
}

fn accept(listener: &UnixListener) -> UnixStream {
    ready(&[listener]);
    let stream = listener.accept().expect("accept barrier").0;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("barrier deadline");
    stream
}

fn line(stream: &mut UnixStream) -> String {
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).expect("barrier line");
    line.trim().to_string()
}

fn publication_barrier(start: UnixListener, blocked: UnixListener, committed: UnixListener) {
    accept(&start)
        .write_all(b"1")
        .expect("worker may attempt agent publication");
    let fds = ready(&[&blocked, &committed]);
    if fds[0].revents & libc::POLLIN != 0 {
        let mut barrier = accept(&blocked);
        assert_eq!(line(&mut barrier), "blocked");
        barrier.write_all(b"1").expect("retry when worker guard commits");
    }
}

fn command(home: &std::path::Path) -> Command {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "shift; \"$@\" & wait \"$!\"", "__run-task", "t-launch"])
        .arg(std::env::current_exe().expect("test binary"))
        .args(["--exact", HELPER, "--ignored", "--nocapture"])
        .env("AID_NO_DETACH", "1")
        .env("AID_LAUNCH_TEST_HOME", home)
        .stdout(Stdio::null())
        .process_group(0);
    cmd
}

fn install_spawn(
    mut cmd: Command,
    start: UnixListener,
    blocked: UnixListener,
    committed: UnixListener,
) -> Rc<RefCell<Option<Worker>>> {
    let spawned = Rc::new(RefCell::new(None));
    let holder = spawned.clone();
    SPAWN.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || {
            let child = cmd.spawn()?;
            let pid = child.id();
            *holder.borrow_mut() = Some(Worker {
                child,
                stopped: false,
            });
            BackgroundRunSpec::on_next_save(move || publication_barrier(start, blocked, committed));
            Ok(pid)
        }));
    });
    spawned
}

#[test]
fn no_detach_publications_preserve_worker_and_agent_pids_and_force_stop_both() {
    let _permit = crate::support::test_subprocess::acquire();
    let (home, _guard, store, _prepared, spec) = setup();
    let agents = UnixListener::bind(home.path().join("agent.sock")).expect("agent barrier");
    let committed = UnixListener::bind(home.path().join("worker.sock")).expect("commit barrier");
    let start = UnixListener::bind(home.path().join("start.sock")).expect("start barrier");
    let blocked = UnixListener::bind(home.path().join("ordering.sock")).expect("ordering barrier");
    let holder = install_spawn(
        command(home.path()),
        start,
        blocked,
        committed.try_clone().expect("clone"),
    );
    let worker_pid = spawn_background_worker(&store, &spec).expect("worker launch");
    let mut worker = holder.borrow_mut().take().expect("worker handle");
    assert_eq!(worker_pid, worker.child.id());
    let mut agent = accept(&agents);
    let pid = line(&mut agent).parse::<u32>().expect("agent PID");
    let _cleanup = AgentCleanup(pid);
    let mut finished = accept(&committed);
    assert_eq!(line(&mut finished), "committed");
    let saved = background::load_spec_if_exists("t-launch").unwrap().unwrap();
    crate::cmd::stop::kill(&store, "t-launch").expect("force stop");
    worker.stopped = true;
    assert_eq!(finished.read(&mut [0]).expect("worker closed"), 0);
    assert_eq!(agent.read(&mut [0]).expect("agent closed"), 0);
    assert_eq!(saved.worker_pid, Some(worker_pid));
    assert_eq!(
        saved.agent_pid,
        Some(pid),
        "worker publication must not erase the agent PID"
    );
    assert!(!crate::support::test_subprocess::is_live(worker_pid as i32));
    assert!(!crate::support::test_subprocess::is_live(pid as i32));
    assert_eq!(
        store.get_task("t-launch").unwrap().unwrap().status,
        TaskStatus::Stopped
    );
    assert!(!paths::job_path("t-launch").exists());
}
