// E2E coverage for PTY idle reaping with pure terminal-control output.
// Verifies CSI spinner noise is not liveness and idle kill details are persisted.
// Deps: compiled `aid` binary, tempfile, rusqlite, and a shell-backed custom agent.

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

const TASK_ID: &str = "t-pty-idle";

fn aid_cmd_in(aid_home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_aid"));
    cmd.env("AID_HOME", aid_home);
    cmd.env("AID_NO_DETACH", "1");
    // Project config is discovered from the working directory. Left at the repo root, this test's
    // dispatched task inherits ai-dispatch's own verify command and runs the whole unit suite as
    // verification, which blows past the idle-reap window this test measures. The temp AID_HOME is
    // not a git repo, so discovery finds nothing and the task is verified against nothing.
    cmd.current_dir(aid_home);
    cmd
}

#[test]
fn pty_csi_spinner_noise_does_not_reset_idle_timeout() {
    if !pty_available() {
        return;
    }
    let aid_home = TempDir::new().unwrap();
    let script_dir = TempDir::new().unwrap();
    let agent_path = write_script(
        script_dir.path(),
        "pty-idle-agent",
        "#!/bin/sh\nprintf '\\033[?25l\\033[?25h'\nsleep 10\n",
    );
    write_custom_agent(aid_home.path(), "ptyidle", &agent_path);

    run_ok(aid_cmd_in(aid_home.path()).args([
        "run",
        "ptyidle",
        "emit only spinner noise",
        "--bg",
        "--id",
        TASK_ID,
        "--idle-timeout",
        "2",
    ]));
    wait_for_status(aid_home.path(), TASK_ID, "failed", Duration::from_secs(10));

    let events = event_details(aid_home.path(), TASK_ID);
    assert!(events.iter().any(|(_, detail, _)| {
        detail.contains("Agent hung: no output for 2 seconds")
    }));
    assert!(events.iter().any(|(_, _, metadata)| {
        metadata.as_deref().is_some_and(|text| text.contains("\"event_count\":0"))
    }));
}

fn pty_available() -> bool {
    cfg!(unix) && Path::new("/dev/ptmx").exists()
}

fn run_ok(cmd: &mut Command) -> std::process::Output {
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn wait_for_status(aid_home: &Path, task_id: &str, expected: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if task_status(aid_home, task_id).as_deref() == Some(expected) {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!(
        "task {task_id} did not reach {expected}; latest status: {:?}",
        task_status(aid_home, task_id)
    );
}

fn task_status(aid_home: &Path, task_id: &str) -> Option<String> {
    let conn = Connection::open(aid_home.join("aid.db")).ok()?;
    conn.query_row(
        "SELECT status FROM tasks WHERE id = ?1",
        [task_id],
        |row| row.get::<_, String>(0),
    )
    .ok()
}

fn event_details(aid_home: &Path, task_id: &str) -> Vec<(String, String, Option<String>)> {
    let conn = Connection::open(aid_home.join("aid.db")).unwrap();
    let mut stmt = conn
        .prepare("SELECT event_type, detail, metadata FROM events WHERE task_id = ?1")
        .unwrap();
    stmt.query_map([task_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(|row| row.unwrap())
        .collect()
}

fn write_custom_agent(aid_home: &Path, id: &str, command: &Path) {
    let agents_dir = aid_home.join("agents");
    std::fs::create_dir_all(&agents_dir).unwrap();
    std::fs::write(
        agents_dir.join(format!("{id}.toml")),
        format!(
            "[agent]\nid = \"{id}\"\ndisplay_name = \"{id}\"\ncommand = \"{}\"\ntrust_tier = \"local\"\nprompt_mode = \"arg\"\nstreaming = true\noutput_format = \"jsonl\"\n",
            command.display()
        ),
    )
    .unwrap();
}

fn write_script(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    #[cfg(unix)]
    {
        let permissions = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
    }
    path
}
