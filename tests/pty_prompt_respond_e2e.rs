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

#[test]
fn buffered_progress_never_awaits() {
    if !pty_available() { return; }
    let aid_home = TempDir::new().unwrap();
    let script_dir = TempDir::new().unwrap();
    let agent_path = write_script(script_dir.path(), "buffered-agent", r#"#!/bin/sh
mkdir -p "$AID_HOME/tasks/t-pty-respond"
printf "I'll start by exploring the codebase structure..."
i=0
while [ "$i" -lt 8 ]; do
  echo working >> "$AID_HOME/tasks/t-pty-respond/agent.log"
  sleep 1
  i=$((i + 1))
done
printf '\nfinished\n'
"#);
    write_custom_agent(aid_home.path(), "buffered", &agent_path);
    run_ok(aid_cmd_in(aid_home.path()).args([
        "run", "buffered", "work in a log", "--bg", "--id", TASK_ID,
    ]));
    wait_for_status(aid_home.path(), TASK_ID, "done", Duration::from_secs(20));
    let conn = Connection::open(aid_home.path().join("aid.db")).unwrap();
    let awaits: i64 = conn.query_row(
        "SELECT COUNT(*) FROM events WHERE task_id = ?1 AND json_extract(metadata, '$.awaiting_input') = 1",
        [TASK_ID], |row| row.get(0),
    ).unwrap();
    assert_eq!(awaits, 0, "buffered progress must never advertise AWAIT");
}

#[test]
fn idle_prompt_with_silent_log_still_accepts_response() {
    if !pty_available() { return; }
    let aid_home = TempDir::new().unwrap();
    let script_dir = TempDir::new().unwrap();
    let agent_path = write_script(script_dir.path(), "silent-prompt-agent", r#"#!/bin/sh
mkdir -p "$AID_HOME/tasks/t-pty-respond"
echo ready > "$AID_HOME/tasks/t-pty-respond/agent.log"
printf 'Select an option'
read answer
printf 'accepted %s\n' "$answer"
"#);
    write_custom_agent(aid_home.path(), "silentprompt", &agent_path);
    run_ok(aid_cmd_in(aid_home.path()).args([
        "run", "silentprompt", "ask for an option", "--bg", "--id", TASK_ID,
    ]));
    wait_for_status(aid_home.path(), TASK_ID, "awaiting_input", Duration::from_secs(10));
    thread::sleep(Duration::from_secs(2));
    assert_eq!(task_status(aid_home.path(), TASK_ID).as_deref(), Some("awaiting_input"));
    run_ok(aid_cmd_in(aid_home.path()).args(["respond", TASK_ID, "one"]));
    wait_for_status(aid_home.path(), TASK_ID, "done", Duration::from_secs(10));
}

#[test]
fn buffered_log_growth_leaves_await_without_stdin_response() {
    if !pty_available() { return; }
    let aid_home = TempDir::new().unwrap();
    let script_dir = TempDir::new().unwrap();
    let agent_path = write_script(script_dir.path(), "resuming-agent", r#"#!/bin/sh
mkdir -p "$AID_HOME/tasks/t-pty-respond"
printf "I'll start by exploring the codebase structure..."
i=0
while [ ! -f "$AID_HOME/resume" ] && [ "$i" -lt 150 ]; do sleep 0.1; i=$((i + 1)); done
echo working >> "$AID_HOME/tasks/t-pty-respond/agent.log"
i=0
while [ ! -f "$AID_HOME/finish" ] && [ "$i" -lt 150 ]; do sleep 0.1; i=$((i + 1)); done
printf '\nfinished\n'
"#);
    write_custom_agent(aid_home.path(), "resuming", &agent_path);
    run_ok(aid_cmd_in(aid_home.path()).args([
        "run", "resuming", "resume work in a log", "--bg", "--id", TASK_ID,
    ]));
    wait_for_status(aid_home.path(), TASK_ID, "awaiting_input", Duration::from_secs(10));
    std::fs::write(aid_home.path().join("resume"), "").unwrap();
    wait_for_status(aid_home.path(), TASK_ID, "running", Duration::from_secs(10));
    thread::sleep(Duration::from_secs(5));
    assert_eq!(task_status(aid_home.path(), TASK_ID).as_deref(), Some("running"));
    let conn = Connection::open(aid_home.path().join("aid.db")).unwrap();
    let recoveries: i64 = conn.query_row(
        "SELECT COUNT(*) FROM events WHERE task_id = ?1 AND json_extract(metadata, '$.reason') = 'agent_log_growth'",
        [TASK_ID], |row| row.get(0),
    ).unwrap();
    assert_eq!(recoveries, 1);
    std::fs::write(aid_home.path().join("finish"), "").unwrap();
    wait_for_status(aid_home.path(), TASK_ID, "done", Duration::from_secs(10));
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
