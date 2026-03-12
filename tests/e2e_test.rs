// E2E tests for aid CLI.
// Tests the binary as a subprocess to verify full command flow.

use std::process::Command;

fn aid_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_aid"))
}

#[test]
fn help_shows_subcommands() {
    let output = aid_cmd().arg("--help").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("run"));
    assert!(stdout.contains("watch"));
    assert!(stdout.contains("board"));
    assert!(stdout.contains("audit"));
    assert!(stdout.contains("agents"));
}

#[test]
fn board_works_with_empty_db() {
    let output = aid_cmd().arg("board").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No tasks found") || stdout.contains("Tasks:"));
}

#[test]
fn agents_detects_installed_clis() {
    let output = aid_cmd().arg("agents").output().unwrap();
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
    let output = aid_cmd()
        .args(["run", "nonexistent", "test prompt"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unknown agent"));
}

#[test]
fn audit_missing_task_fails() {
    let output = aid_cmd()
        .args(["audit", "t-9999"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found"));
}

#[test]
fn version_flag_works() {
    let output = aid_cmd().arg("--version").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("aid"));
}
