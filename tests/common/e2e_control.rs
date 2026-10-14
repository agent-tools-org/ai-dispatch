// CLI control flows for groups, MCP requests and queued responses.
// Retains the existing E2E assertions using isolated aid homes and the compiled CLI.
use super::{aid_cmd, aid_cmd_in};
use rusqlite::params;
use std::io::Write;
use std::process::Stdio;
use tempfile::{NamedTempFile, TempDir};

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
    let group_id = stdout.trim().to_string();
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
    let group_id = create_stdout.trim().to_string();

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

    assert_group_deleted(&temp_dir, &group_id);
}

fn assert_group_deleted(temp_dir: &TempDir, group_id: &str) {
    let delete_output = aid_cmd_in(temp_dir.path())
        .args(["group", "delete", group_id])
        .output()
        .unwrap();
    assert!(delete_output.status.success());
    let delete_stdout = String::from_utf8_lossy(&delete_output.stdout);
    assert!(delete_stdout.contains("deleted"));
    assert!(
        delete_stdout
            .contains("Historical tasks still tagged: 0 — use --cascade to also delete them")
    );

    let list_output = aid_cmd_in(temp_dir.path())
        .args(["group", "list"])
        .output()
        .unwrap();
    assert!(list_output.status.success());
    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(!list_stdout.contains("dispatch-core"));

    let deleted_show = aid_cmd_in(temp_dir.path())
        .args(["group", "show", group_id])
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

#[test]
fn respond_reads_response_text_from_file() {
    let temp_dir = TempDir::new().unwrap();
    let mut response_file = NamedTempFile::new().unwrap();
    write!(response_file, "text with `backticks` and {{braces}}").unwrap();

    // `aid respond` now refuses to queue input for a task it cannot find, so the row
    // has to exist — and it has to belong to an agent that reads stdin.
    let init = aid_cmd_in(temp_dir.path()).arg("board").output().unwrap();
    assert!(init.status.success());
    let conn = rusqlite::Connection::open(temp_dir.path().join("aid.db")).unwrap();
    conn.execute(
        "INSERT INTO tasks (id, agent, prompt, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            "t-bb02",
            "gemini",
            "awaiting input",
            "running",
            "2026-08-10T00:00:00+00:00"
        ],
    )
    .unwrap();

    let output = aid_cmd_in(temp_dir.path())
        .args([
            "respond",
            "t-bb02",
            "--file",
            response_file.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let queued = std::fs::read_to_string(temp_dir.path().join("jobs/t-bb02.input")).unwrap();
    assert_eq!(queued, "text with `backticks` and {braces}");
}

#[test]
fn test_clear_limit_unknown_agent() {
    let (mut cmd, _tmp) = aid_cmd();
    let output = cmd
        .args(["config", "clear-limit", "unknown_agent_xyz"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn test_clear_limit_codex() {
    let (mut cmd, _tmp) = aid_cmd();
    let output = cmd
        .args(["config", "clear-limit", "codex"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("not rate-limited"));
}
