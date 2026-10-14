// E2E coverage for PTY streaming agent output with terminal escapes.
// Verifies OSC/CSI-prefixed JSONL events are stripped, persisted, and shown cleanly.
// Deps: compiled `aid` binary, tempfile, rusqlite, and a shell-backed custom agent.

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

const TASK_ID: &str = "t-pty-osc";

mod common;
use common::aid_cmd_in;

#[test]
fn pty_streaming_agent_strips_osc_and_persists_events() {
    if !pty_available() {
        return;
    }
    let aid_home = TempDir::new().unwrap();
    let script_dir = TempDir::new().unwrap();
    let agent_path = write_script(
        script_dir.path(),
        "pty-osc-agent",
        "#!/bin/sh\n\
         printf '\\033]0;\\342\\233\\254 reply pong\\007{\"type\":\"system\",\"subtype\":\"init\"}\\n'\n\
         printf '\\033]9;4;0;\\007\\033]9;4;3;\\007{\"type\":\"message\",\"role\":\"user\",\"text\":\"hi\"}\\n'\n\
         printf '\\033]9;4;0;\\007{\"type\":\"completion\",\"finalText\":\"pong\"}\\n'\n",
    );
    write_custom_agent(aid_home.path(), "ptyosc", &agent_path, true);

    run_ok(aid_cmd_in(aid_home.path()).args([
        "run",
        "ptyosc",
        "exercise pty stream",
        "--bg",
        "--id",
        TASK_ID,
    ]));
    wait_for_status(aid_home.path(), TASK_ID, "done", Duration::from_secs(10));

    let show = run_ok(aid_cmd_in(aid_home.path()).args(["show", TASK_ID, "--output"]));
    assert!(
        !show.stdout.contains(&0x1b),
        "show --output leaked ESC bytes: {:?}",
        String::from_utf8_lossy(&show.stdout)
    );

    let events = event_details(aid_home.path(), TASK_ID);
    assert!(events.iter().any(|(kind, detail)| kind == "reasoning" && detail == "hi"));
    assert!(events.iter().any(|(kind, _)| kind == "completion"));
}

#[test]
fn reply_write_and_echo_do_not_claim_consumption() {
    assert!(pty_available(), "this regression requires the remote PTY test host");
    let home = TempDir::new().unwrap();
    let scripts = TempDir::new().unwrap();
    let agent = write_script(scripts.path(), "input-agent",
        "#!/bin/sh\nread line\nprintf 'unrelated output\\n'\n");
    write_custom_agent(home.path(), "inputagent", &agent, false);
    run_ok(aid_cmd_in(home.path()).args([
        "run", "inputagent", "wait for input", "--bg", "--id", TASK_ID,
    ]));
    let reply = run_ok(aid_cmd_in(home.path()).args([
        "reply", TASK_ID, "pivot", "--timeout", "10",
    ]));
    assert!(String::from_utf8_lossy(&reply.stdout)
        .contains("written to PTY; consumption unconfirmed"));
    wait_for_status(home.path(), TASK_ID, "done", Duration::from_secs(10));
    let log = std::fs::read_to_string(home.path().join(format!("logs/{TASK_ID}.jsonl"))).unwrap();
    assert!(log.contains("pivot"), "terminal echo must exercise the output path");
    assert!(log.contains("unrelated output"));
    let events = event_details(home.path(), TASK_ID);
    assert!(!events.iter().any(|(_, detail)| detail.contains("Acked reply")));
    assert!(events.iter().any(|(_, detail)| detail.contains("consumption unconfirmed")));
    let db = Connection::open(home.path().join("aid.db")).unwrap();
    let delivered: i64 = db.query_row(
        "SELECT count(*) FROM task_messages WHERE delivered_at IS NOT NULL", [],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(delivered, 1);
    let ack_columns: i64 = db.query_row(
        "SELECT count(*) FROM pragma_table_info('task_messages') WHERE name = 'acked_at'", [],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(ack_columns, 1);
    let acked_at: Option<String> = db.query_row(
        "SELECT acked_at FROM task_messages", [], |row| row.get(0),
    ).unwrap();
    assert!(acked_at.is_none());
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

fn event_details(aid_home: &Path, task_id: &str) -> Vec<(String, String)> {
    let conn = Connection::open(aid_home.join("aid.db")).unwrap();
    let mut stmt = conn
        .prepare("SELECT event_type, detail FROM events WHERE task_id = ?1")
        .unwrap();
    stmt.query_map([task_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(|row| row.unwrap())
        .collect()
}

fn write_custom_agent(aid_home: &Path, id: &str, command: &Path, streaming: bool) {
    let agents_dir = aid_home.join("agents");
    std::fs::create_dir_all(&agents_dir).unwrap();
    std::fs::write(
        agents_dir.join(format!("{id}.toml")),
        format!(
            "[agent]\nid = \"{id}\"\ndisplay_name = \"{id}\"\ncommand = \"{}\"\ntrust_tier = \"local\"\nstreaming = {streaming}\noutput_format = \"jsonl\"\n",
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
