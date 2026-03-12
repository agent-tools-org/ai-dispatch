// E2E tests for aid CLI.
// Tests the binary as a subprocess to verify full command flow.

use std::process::Command;
use std::path::Path;
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
    assert!(stdout.contains("wait"));
    assert!(stdout.contains("board"));
    assert!(stdout.contains("audit"));
    assert!(stdout.contains("output"));
    assert!(stdout.contains("group"));
    assert!(stdout.contains("usage"));
    assert!(stdout.contains("agents"));
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
fn wait_works_with_empty_db() {
    let (mut cmd, _tmp) = aid_cmd();
    let output = cmd.arg("wait").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No running tasks"));
}

#[test]
fn agents_detects_installed_clis() {
    let (mut cmd, _tmp) = aid_cmd();
    let output = cmd.arg("agents").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // At least one of these should be detected in the dev environment
    assert!(
        stdout.contains("gemini") || stdout.contains("codex")
            || stdout.contains("opencode") || stdout.contains("No AI CLI agents"),
    );
}

#[test]
fn run_unknown_agent_fails() {
    let (mut cmd, _tmp) = aid_cmd();
    let output = cmd.args(["run", "nonexistent", "test prompt"]).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unknown agent"));
}

#[test]
fn audit_missing_task_fails() {
    let (mut cmd, _tmp) = aid_cmd();
    let output = cmd.args(["audit", "t-9999"]).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found"));
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
        .args(["group", "create", "dispatch", "--context", "Shared repo rules."])
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
        .args(["group", "create", "dispatch", "--context", "Shared repo rules."])
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
