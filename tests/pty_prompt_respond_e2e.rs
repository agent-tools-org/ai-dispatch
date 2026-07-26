// E2E coverage for PTY prompt detection and file-backed responses.
// Verifies watch exits on awaiting input and `aid respond` unblocks the agent.
// Deps: compiled `aid` binary, tempfile, rusqlite, and a shell-backed custom agent.

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

const TASK_ID: &str = "t-pty-respond";

mod common;
use common::aid_cmd_in;

#[test]
fn pty_prompt_response_unblocks_background_agent() {
    if !pty_available() {
        return;
    }
    let aid_home = TempDir::new().unwrap();
    let script_dir = TempDir::new().unwrap();
    let agent_path = write_script(
        script_dir.path(),
        "pty-prompt-agent",
        "#!/bin/sh\nprintf 'Proceed? (y/n) '\nread answer\nprintf 'accepted %s\\n' \"$answer\"\n",
    );
    write_custom_agent(aid_home.path(), "ptyprompt", &agent_path);

    run_ok(aid_cmd_in(aid_home.path()).args([
        "run",
        "ptyprompt",
        "ask before proceeding",
        "--bg",
        "--id",
        TASK_ID,
    ]));
    wait_for_status(aid_home.path(), TASK_ID, "awaiting_input", Duration::from_secs(10));

    let watch = run_ok(aid_cmd_in(aid_home.path()).args([
        "watch",
        "--quiet",
        "--exit-on-await",
        TASK_ID,
    ]));
    let watch_stdout = String::from_utf8_lossy(&watch.stdout);
    assert!(watch_stdout.contains(TASK_ID));
    assert!(watch_stdout.contains("Proceed? (y/n)"));

    run_ok(aid_cmd_in(aid_home.path()).args(["respond", TASK_ID, "y"]));
    wait_for_status(aid_home.path(), TASK_ID, "done", Duration::from_secs(10));

    let log = std::fs::read_to_string(aid_home.path().join(format!("logs/{TASK_ID}.jsonl")))
        .unwrap_or_default();
    assert!(log.contains("accepted y"));
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

fn write_custom_agent(aid_home: &Path, id: &str, command: &Path) {
    let agents_dir = aid_home.join("agents");
    std::fs::create_dir_all(&agents_dir).unwrap();
    std::fs::write(
        agents_dir.join(format!("{id}.toml")),
        format!(
            "[agent]\nid = \"{id}\"\ndisplay_name = \"{id}\"\ncommand = \"{}\"\ntrust_tier = \"local\"\nprompt_mode = \"arg\"\nstreaming = false\n",
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
