// E2E tests for aid CLI.
// Tests the binary as a subprocess to verify full command flow.

use rusqlite::params;
use std::process::Command;
use tempfile::TempDir;

#[path = "common/e2e_batch.rs"]
#[cfg(unix)]
mod batch;
mod common;
#[path = "common/e2e_control.rs"]
mod control;
use common::aid_cmd_in;

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
    assert!(stdout.contains("merge"));
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
fn board_rapid_calls_show_data_without_exit() {
    let (mut cmd, _tmp) = aid_cmd();
    let aid_home = _tmp.path().to_path_buf();

    // First board call - creates marker file
    let output1 = cmd.arg("board").output().unwrap();
    assert!(output1.status.success());

    // Second board call within 10s with same state (empty)
    // Reset marker timestamp to old value so --force cooldown (30s) is satisfied
    let marker = aid_home.join("board-last.txt");
    if marker.exists() {
        let content = std::fs::read_to_string(&marker).unwrap();
        let patched = format!("0\n{}", content.split_once('\n').map(|x| x.1).unwrap_or(""));
        std::fs::write(&marker, patched).unwrap();
    }
    let mut cmd2 = aid_cmd_in(&aid_home);
    let output2 = cmd2.args(["board", "--force"]).output().unwrap();

    // Must succeed with --force
    assert!(
        output2.status.success(),
        "board --force should bypass cooldown"
    );
    let stdout = String::from_utf8_lossy(&output2.stdout);
    // Board shows data even when called rapidly
    assert!(stdout.contains("No tasks found") || stdout.contains("Tasks:"));
}

#[test]
fn notifications_prints_recent_lines() {
    let temp_dir = TempDir::new().unwrap();
    std::fs::write(
        temp_dir.path().join("completions.jsonl"),
        "{\"task_id\":\"t-1\"}\n{\"task_id\":\"t-2\"}\n",
    )
    .unwrap();

    let output = aid_cmd_in(temp_dir.path())
        .arg("notifications")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "{\"task_id\":\"t-1\"}\n{\"task_id\":\"t-2\"}\n",
    );
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
fn agent_fork_creates_builtin_toml() {
    let temp_dir = TempDir::new().unwrap();
    let output = aid_cmd_in(temp_dir.path())
        .args(["agent", "fork", "codex"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let agent_path = temp_dir.path().join("agents").join("codex-custom.toml");
    let contents = std::fs::read_to_string(&agent_path).unwrap();
    assert!(contents.contains("command = \"codex\""));
    assert!(contents.contains("prompt_mode = \"arg\""));
    assert!(contents.contains("[agent.capabilities]"));
    // The fork carries user-declared capability keys, not the removed per-CLI matrix row
    // (codex was research = 1, complex_impl = 9 there).
    for key in [
        "research = ",
        "simple_edit = ",
        "complex_impl = ",
        "frontend = ",
        "debugging = ",
        "testing = ",
        "refactoring = ",
        "documentation = ",
    ] {
        assert!(contents.contains(key), "missing {key}");
    }
    assert!(!contents.contains("complex_impl = 9"));
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
fn merge_marks_done_task_as_merged() {
    let temp_dir = TempDir::new().unwrap();
    let init = aid_cmd_in(temp_dir.path()).arg("board").output().unwrap();
    assert!(init.status.success());

    let conn = rusqlite::Connection::open(temp_dir.path().join("aid.db")).unwrap();
    let created_at = "2026-03-13T00:00:00+00:00";
    conn.execute(
        "INSERT INTO tasks (id, agent, prompt, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params!["t-2001", "codex", "merge me", "done", created_at],
    )
    .unwrap();

    let merge_output = aid_cmd_in(temp_dir.path())
        .args(["merge", "t-2001"])
        .output()
        .unwrap();
    assert!(merge_output.status.success());
    let merge_stdout = String::from_utf8_lossy(&merge_output.stdout);
    assert!(merge_stdout.contains("Marked t-2001 as merged"));

    // Reset marker timestamp so --force cooldown (30s) is satisfied
    let marker = temp_dir.path().join("board-last.txt");
    if marker.exists() {
        let content = std::fs::read_to_string(&marker).unwrap();
        let patched = format!("0\n{}", content.split_once('\n').map(|x| x.1).unwrap_or(""));
        std::fs::write(&marker, patched).unwrap();
    }
    let board_output = aid_cmd_in(temp_dir.path())
        .args(["board", "--force"])
        .output()
        .unwrap();
    assert!(board_output.status.success());
    let board_stdout = String::from_utf8_lossy(&board_output.stdout);
    assert!(board_stdout.contains("t-2001"));
    assert!(board_stdout.contains("MERGED"));
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
