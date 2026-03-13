// E2E tests for aid CLI.
// Tests the binary as a subprocess to verify full command flow.

use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;
use rusqlite::params;
use tempfile::TempDir;

fn aid_cmd_in(aid_home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_aid"));
    cmd.env("AID_HOME", aid_home);
    cmd
}

fn aid_cmd() -> (Command, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let cmd = aid_cmd_in(temp_dir.path());
    (cmd, temp_dir)
}

#[test]
fn help_shows_subcommands() {
    let (mut cmd, _tmp) = aid_cmd();
    let output = cmd.arg("--help").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("run"));
    assert!(stdout.contains("watch"));
    assert!(stdout.contains("board"));
    assert!(stdout.contains("show"));
    assert!(stdout.contains("ask"));
    assert!(stdout.contains("group"));
    assert!(stdout.contains("usage"));
    assert!(stdout.contains("config"));
}

#[test]
fn board_works_with_empty_db() {
    let (mut cmd, _tmp) = aid_cmd();
    let output = cmd.arg("board").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No tasks found") || stdout.contains("Tasks:"));
}

#[test]
fn completions_prints_recent_lines() {
    let temp_dir = TempDir::new().unwrap();
    std::fs::write(
        temp_dir.path().join("completions.jsonl"),
        "{\"task_id\":\"t-1\"}\n{\"task_id\":\"t-2\"}\n",
    )
    .unwrap();

    let output = aid_cmd_in(temp_dir.path())
        .arg("completions")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "{\"task_id\":\"t-1\"}\n{\"task_id\":\"t-2\"}\n",
    );
}

#[test]
fn watch_quiet_works_with_empty_db() {
    let (mut cmd, _tmp) = aid_cmd();
    let output = cmd.args(["watch", "--quiet"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No running tasks"));
}

#[test]
fn config_agents_detects_installed_clis() {
    let (mut cmd, _tmp) = aid_cmd();
    let output = cmd.args(["config", "agents"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // At least one of these should be detected in the dev environment
    assert!(
        stdout.contains("gemini")
            || stdout.contains("codex")
            || stdout.contains("opencode")
            || stdout.contains("No AI CLI agents"),
    );
}

#[test]
fn run_unknown_agent_fails() {
    let (mut cmd, _tmp) = aid_cmd();
    let output = cmd
        .args(["run", "nonexistent", "test prompt"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unknown agent"));
}

#[test]
fn show_missing_task_fails() {
    let (mut cmd, _tmp) = aid_cmd();
    let output = cmd.args(["show", "t-9999"]).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found"));
}

#[test]
fn show_displays_retry_chain_history() {
    let temp_dir = TempDir::new().unwrap();
    let init = aid_cmd_in(temp_dir.path()).arg("board").output().unwrap();
    assert!(init.status.success());

    let conn = rusqlite::Connection::open(temp_dir.path().join("aid.db")).unwrap();
    let created_at = "2026-03-13T00:00:00+00:00";
    conn.execute(
        "INSERT INTO tasks (id, agent, prompt, status, parent_task_id, duration_ms, cost_usd, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params!["t-1001", "codex", "root task", "done", Option::<String>::None, 12000, 0.03, created_at],
    ).unwrap();
    conn.execute(
        "INSERT INTO tasks (id, agent, prompt, status, parent_task_id, duration_ms, cost_usd, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params!["t-1002", "codex", "retry task", "failed", "t-1001", 8000, 0.02, created_at],
    ).unwrap();
    conn.execute(
        "INSERT INTO tasks (id, agent, prompt, status, parent_task_id, duration_ms, cost_usd, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params!["t-1003", "codex", "current task", "done", "t-1002", 15000, 0.04, created_at],
    ).unwrap();

    let output = aid_cmd_in(temp_dir.path())
        .args(["show", "t-1003"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Retry chain:"));
    assert!(stdout.contains("t-1001 (root)  → Done"));
    assert!(stdout.contains("t-1002 (retry)  → Failed"));
    assert!(stdout.contains("t-1003 (retry)  → Done"));
    assert!(stdout.contains("← current"));
}

#[test]
fn version_flag_works() {
    let (mut cmd, _tmp) = aid_cmd();
    let output = cmd.arg("--version").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("aid"));
}

#[test]
fn group_create_list_and_show_work() {
    let temp_dir = TempDir::new().unwrap();
    let output = aid_cmd_in(temp_dir.path())
        .args([
            "group",
            "create",
            "dispatch",
            "--context",
            "Shared repo rules.",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let group_id = stdout.split_whitespace().nth(1).unwrap().to_string();
    assert!(group_id.starts_with("wg-"));

    let list_output = aid_cmd_in(temp_dir.path())
        .args(["group", "list"])
        .output()
        .unwrap();
    assert!(list_output.status.success());
    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(list_stdout.contains("dispatch"));
    assert!(list_stdout.contains(&group_id));

    let show_output = aid_cmd_in(temp_dir.path())
        .args(["group", "show", &group_id])
        .output()
        .unwrap();
    assert!(show_output.status.success());
    let show_stdout = String::from_utf8_lossy(&show_output.stdout);
    assert!(show_stdout.contains("Shared repo rules."));
    assert!(show_stdout.contains("(none)"));
}

#[test]
fn group_update_and_delete_work() {
    let temp_dir = TempDir::new().unwrap();
    let create_output = aid_cmd_in(temp_dir.path())
        .args([
            "group",
            "create",
            "dispatch",
            "--context",
            "Shared repo rules.",
        ])
        .output()
        .unwrap();
    assert!(create_output.status.success());
    let create_stdout = String::from_utf8_lossy(&create_output.stdout);
    let group_id = create_stdout.split_whitespace().nth(1).unwrap().to_string();

    let update_output = aid_cmd_in(temp_dir.path())
        .args([
            "group",
            "update",
            &group_id,
            "--name",
            "dispatch-core",
            "--context",
            "Updated rollout notes.",
        ])
        .output()
        .unwrap();
    assert!(update_output.status.success());
    let update_stdout = String::from_utf8_lossy(&update_output.stdout);
    assert!(update_stdout.contains("dispatch-core"));
    assert!(update_stdout.contains("Updated rollout notes."));

    let show_output = aid_cmd_in(temp_dir.path())
        .args(["group", "show", &group_id])
        .output()
        .unwrap();
    assert!(show_output.status.success());
    let show_stdout = String::from_utf8_lossy(&show_output.stdout);
    assert!(show_stdout.contains("dispatch-core"));
    assert!(show_stdout.contains("Updated rollout notes."));

    let delete_output = aid_cmd_in(temp_dir.path())
        .args(["group", "delete", &group_id])
        .output()
        .unwrap();
    assert!(delete_output.status.success());
    let delete_stdout = String::from_utf8_lossy(&delete_output.stdout);
    assert!(delete_stdout.contains("deleted"));
    assert!(delete_stdout.contains("Historical tasks still tagged: 0"));

    let list_output = aid_cmd_in(temp_dir.path())
        .args(["group", "list"])
        .output()
        .unwrap();
    assert!(list_output.status.success());
    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(!list_stdout.contains("dispatch-core"));

    let deleted_show = aid_cmd_in(temp_dir.path())
        .args(["group", "show", &group_id])
        .output()
        .unwrap();
    assert!(!deleted_show.status.success());
    let deleted_stderr = String::from_utf8_lossy(&deleted_show.stderr);
    assert!(deleted_stderr.contains("not found"));
}

#[test]
fn mcp_tools_list_works_over_stdio_jsonrpc() {
    let temp_dir = TempDir::new().unwrap();
    let mut child = aid_cmd_in(temp_dir.path())
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let stdin = child.stdin.as_mut().unwrap();
    stdin
        .write_all(br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
        .unwrap();
    drop(child.stdin.take());

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("aid_run"));
    assert!(stdout.contains("aid_usage"));
}
