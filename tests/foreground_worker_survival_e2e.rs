// E2E coverage for foreground worker detachment under a caller timeout.
// Verifies real worker and agent survival, completion persistence, and output status.
// Deps: compiled aid binary, tempfile, rusqlite, libc, and a shell-backed agent.

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const TASK_ID: &str = "t-foreground-survivor";
const INTERRUPT_TASK_ID: &str = "t-foreground-interrupt";

#[test]
#[cfg(unix)]
fn foreground_success_and_failure_preserve_status_output_contract() {
    let success = run_foreground_case(
        "success",
        "printf '%s\\n' '{\"type\":\"completion\",\"finalText\":\"done\"}'\n",
    );
    assert!(
        success.status.success(),
        "success status={:?} stdout={} stderr={}",
        success.status,
        String::from_utf8_lossy(&success.stdout),
        String::from_utf8_lossy(&success.stderr)
    );
    assert!(String::from_utf8_lossy(&success.stdout).contains("Task t-foreground-success started"));
    assert!(String::from_utf8_lossy(&success.stdout).contains("[STATUS=DONE]"));
    assert!(String::from_utf8_lossy(&success.stderr).contains("View in TUI: aid board"));

    let failure = run_foreground_case("failure", "exit 1\n");
    assert_eq!(failure.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&failure.stdout).contains("Task t-foreground-failure started"));
    assert!(String::from_utf8_lossy(&failure.stdout).contains("[STATUS=FAILED]"));
    assert!(String::from_utf8_lossy(&failure.stderr).contains("Next: aid show"));
}

#[test]
#[cfg(unix)]
fn foreground_worker_survives_caller_process_tree_kill() {
    let aid_home = TempDir::new().unwrap();
    let script_dir = TempDir::new().unwrap();
    let project_dir = TempDir::new().unwrap();
    let script = write_script(
        script_dir.path(),
        "survivor-agent",
        "#!/bin/sh\n\
         printf '{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"working\"}\\n'\n\
         sleep 2\n\
         printf '{\"type\":\"completion\",\"finalText\":\"done\"}\\n'\n",
    );
    write_custom_agent(aid_home.path(), "survivor", &script);

    let mut cleanup = ProcessCleanup::default();
    let mut aid = Command::new(env!("CARGO_BIN_EXE_aid"));
    aid.env("AID_HOME", aid_home.path())
        .current_dir(project_dir.path())
        .args([
            "run", "survivor", "survive caller timeout", "--id", TASK_ID,
            "--dir", project_dir.path().to_str().unwrap(), "--timeout", "10",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = aid.spawn().unwrap();
    cleanup.track_caller(child.id());
    let (worker_pid, agent_pid) = wait_for_pids(aid_home.path(), Duration::from_secs(5));
    cleanup.track(worker_pid);
    cleanup.track(agent_pid);

    let parent_pid = child.id().to_string();
    let _ = Command::new("pkill")
        .args(["-TERM", "-P", &parent_pid])
        .status();
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    let status = child.wait().unwrap();
    cleanup.disarm_caller();
    assert_eq!(status.code(), Some(143));
    assert!(pid_alive(worker_pid), "worker died with foreground aid");
    assert!(pid_alive(agent_pid), "agent died with foreground aid");

    wait_for_status(aid_home.path(), "done", Duration::from_secs(5));
    assert!(has_completion(aid_home.path()));
    wait_for_no_task_processes(worker_pid, agent_pid, Duration::from_secs(5));
}

#[test]
#[cfg(unix)]
fn foreground_sigint_stops_the_task() {
    let aid_home = TempDir::new().unwrap();
    let script_dir = TempDir::new().unwrap();
    let project_dir = TempDir::new().unwrap();
    let script = write_script(script_dir.path(), "interrupt-agent", "#!/bin/sh\nsleep 10\n");
    write_custom_agent(aid_home.path(), "interrupt", &script);
    let mut cleanup = ProcessCleanup::default();
    let mut child = Command::new(env!("CARGO_BIN_EXE_aid"))
        .env("AID_HOME", aid_home.path())
        .current_dir(project_dir.path())
        .args([
            "run", "interrupt", "stop on interrupt", "--id", INTERRUPT_TASK_ID,
            "--dir", project_dir.path().to_str().unwrap(), "--timeout", "30",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    cleanup.track_caller(child.id());
    let (worker_pid, agent_pid) = wait_for_pids_for(
        aid_home.path(), INTERRUPT_TASK_ID, Duration::from_secs(5),
    );
    cleanup.track(worker_pid);
    cleanup.track(agent_pid);
    unsafe {
        libc::kill(child.id() as i32, libc::SIGINT);
    }
    assert_eq!(child.wait().unwrap().code(), Some(130));
    cleanup.disarm_caller();
    wait_for_status_for(aid_home.path(), INTERRUPT_TASK_ID, "stopped", Duration::from_secs(3));
    assert!(!pid_alive(worker_pid), "worker survived SIGINT");
    assert!(!pid_alive(agent_pid), "agent survived SIGINT");
}

#[cfg(unix)]
fn run_foreground_case(agent: &str, script: &str) -> std::process::Output {
    let aid_home = TempDir::new().unwrap();
    let script_dir = TempDir::new().unwrap();
    let project_dir = TempDir::new().unwrap();
    let path = write_script(script_dir.path(), agent, &format!("#!/bin/sh\n{script}"));
    write_custom_agent(aid_home.path(), agent, &path);
    let id = format!("t-foreground-{agent}");
    Command::new(env!("CARGO_BIN_EXE_aid"))
        .env("AID_HOME", aid_home.path())
        .current_dir(project_dir.path())
        .args(["run", agent, "exercise foreground output", "--id", &id, "--dir", project_dir.path().to_str().unwrap()])
        .output()
        .unwrap()
}

#[cfg(unix)]
fn write_custom_agent(aid_home: &Path, id: &str, script: &Path) {
    let agents_dir = aid_home.join("agents");
    std::fs::create_dir_all(&agents_dir).unwrap();
    std::fs::write(
        agents_dir.join(format!("{id}.toml")),
        format!(
            "[agent]\nid = \"{id}\"\ndisplay_name = \"{id}\"\ncommand = \"{}\"\ntrust_tier = \"local\"\nstreaming = true\noutput_format = \"jsonl\"\n",
            script.display(),
        ),
    )
    .unwrap();
}

#[cfg(unix)]
fn write_script(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
fn wait_for_pids(aid_home: &Path, timeout: Duration) -> (u32, u32) {
    wait_for_pids_for(aid_home, TASK_ID, timeout)
}

#[cfg(unix)]
fn wait_for_pids_for(aid_home: &Path, task_id: &str, timeout: Duration) -> (u32, u32) {
    let deadline = Instant::now() + timeout;
    let path = aid_home.join("jobs").join(format!("{task_id}.json"));
    while Instant::now() < deadline {
        if let Ok(value) = std::fs::read_to_string(&path) {
            let json: serde_json::Value = serde_json::from_str(&value).unwrap();
            if let (Some(worker), Some(agent)) = (
                json["worker_pid"].as_u64(),
                json["agent_pid"].as_u64(),
            ) {
                return (worker as u32, agent as u32);
            }
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("worker and agent PIDs were not persisted");
}

#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(unix)]
#[derive(Default)]
struct ProcessCleanup {
    caller_pid: Option<u32>,
    task_pids: Vec<u32>,
}

#[cfg(unix)]
impl ProcessCleanup {
    fn track_caller(&mut self, pid: u32) {
        self.caller_pid = Some(pid);
    }

    fn track(&mut self, pid: u32) {
        self.task_pids.push(pid);
    }

    fn disarm_caller(&mut self) {
        self.caller_pid = None;
    }
}

#[cfg(unix)]
impl Drop for ProcessCleanup {
    fn drop(&mut self) {
        if let Some(pid) = self.caller_pid.take() {
            unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        }
        for pid in self.task_pids.drain(..) {
            unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        }
    }
}

#[cfg(unix)]
fn wait_for_no_task_processes(worker_pid: u32, agent_pid: u32, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !pid_alive(worker_pid) && !pid_alive(agent_pid) {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("terminal task left worker {worker_pid} or agent {agent_pid} alive");
}

fn wait_for_status(aid_home: &Path, expected: &str, timeout: Duration) {
    wait_for_status_for(aid_home, TASK_ID, expected, timeout)
}

fn wait_for_status_for(aid_home: &Path, task_id: &str, expected: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if task_status_for(aid_home, task_id).as_deref() == Some(expected) {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("task did not reach {expected}; status: {:?}", task_status_for(aid_home, task_id));
}

fn task_status(aid_home: &Path) -> Option<String> {
    task_status_for(aid_home, TASK_ID)
}

fn task_status_for(aid_home: &Path, task_id: &str) -> Option<String> {
    let conn = Connection::open(aid_home.join("aid.db")).ok()?;
    conn.query_row(
        "SELECT status FROM tasks WHERE id = ?1",
        [task_id],
        |row| row.get::<_, String>(0),
    )
    .ok()
}

fn has_completion(aid_home: &Path) -> bool {
    let conn = Connection::open(aid_home.join("aid.db")).unwrap();
    conn.query_row(
        "SELECT COUNT(*) FROM events WHERE task_id = ?1 AND event_type = 'completion'",
        [TASK_ID],
        |row| row.get::<_, i64>(0),
    )
    .unwrap()
        > 0
}
