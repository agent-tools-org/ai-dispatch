// E2E coverage for the single CLI path per operation.
// Exercises rejection errors, surviving handlers, and quiet watch routing.
// Deps: compiled aid, isolated AID_HOME, rusqlite, serde_json, tempfile.

use rusqlite::{Connection, params};
use std::path::Path;
use tempfile::TempDir;

mod common;
use common::aid_cmd_in;

fn run_ok(home: &Path, args: &[&str]) -> String {
    let output = aid_cmd_in(home).args(args).output().unwrap();
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

fn seeded_home(status: &str) -> TempDir {
    let home = TempDir::new().unwrap();
    run_ok(home.path(), &["board"]);
    let conn = Connection::open(home.path().join("aid.db")).unwrap();
    conn.execute(
        "INSERT INTO tasks (id, agent, prompt, status, created_at)
         VALUES ('t-cli-paths', 'codex', 'test CLI path', ?1, ?2)",
        params![status, chrono::Local::now().to_rfc3339()],
    ).unwrap();
    home
}

#[test]
fn removed_commands_report_clap_unknown_subcommand() {
    let home = TempDir::new().unwrap();
    for (args, verb) in [
        (vec!["kill", "t-cli-paths"], "kill"),
        (vec!["summary", "wg-a"], "summary"),
        (vec!["finding", "list", "wg-a"], "finding"),
        (vec!["broadcast", "wg-a", "message"], "broadcast"),
        (vec!["output", "t-cli-paths"], "output"),
        (vec!["config", "add-agent", "local", "./local"], "add-agent"),
    ] {
        let output = aid_cmd_in(home.path()).args(&args).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(&format!("unrecognized subcommand '{verb}'")), "{stderr}");
    }
}

#[test]
fn watch_wait_flag_is_rejected_by_the_binary() {
    let home = TempDir::new().unwrap();
    let output = aid_cmd_in(home.path()).args(["watch", "--wait", "t-cli-paths"])
        .output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected argument '--wait'"));
}

#[test]
fn quiet_flags_preserve_live_watch_behavior() {
    let home = seeded_home("failed");
    for args in [
        vec!["watch", "t-cli-paths"],
        vec!["watch", "-q", "t-cli-paths"],
        vec!["watch", "--quiet", "t-cli-paths"],
        vec!["-q", "watch", "t-cli-paths"],
    ] {
        let stdout = run_ok(home.path(), &args);
        assert!(stdout.contains("Exiting watch."), "{stdout}");
        assert!(!stdout.contains("Waiting for"), "{stdout}");
    }
    let wait = aid_cmd_in(home.path()).args(["wait", "t-cli-paths"]).output().unwrap();
    assert_eq!(wait.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&wait.stderr).contains("did not succeed"));
}

#[test]
fn stop_force_invokes_forced_termination() {
    let home = seeded_home("waiting");
    let stdout = run_ok(home.path(), &["stop", "t-cli-paths", "--force"]);
    assert!(stdout.contains("Killed t-cli-paths"));
    let conn = Connection::open(home.path().join("aid.db")).unwrap();
    let status: String = conn.query_row(
        "SELECT status FROM tasks WHERE id = 't-cli-paths'", [], |row| row.get(0),
    ).unwrap();
    assert_eq!(status, "stopped");
}

#[test]
fn show_output_full_preserves_long_messages() {
    let home = seeded_home("done");
    let text = format!("START {} END", "complete output ".repeat(800));
    let log_path = home.path().join("logs/t-cli-paths.jsonl");
    std::fs::write(&log_path, serde_json::json!({"type": "result", "finalText": text}).to_string()).unwrap();
    let conn = Connection::open(home.path().join("aid.db")).unwrap();
    conn.execute("UPDATE tasks SET log_path = ?1 WHERE id = 't-cli-paths'", params![log_path.to_str().unwrap()]).unwrap();
    let stdout = run_ok(home.path(), &["show", "t-cli-paths", "--output", "--full"]);
    assert!(stdout.contains(&text), "full output was truncated");
}

#[test]
fn group_paths_preserve_findings_summary_and_broadcast() {
    let home = TempDir::new().unwrap();
    let group = run_ok(home.path(), &["group", "create", "CLI paths"]);
    let group = group.trim();
    run_ok(home.path(), &[
        "group", "finding", "add", group, "Evidence survives", "--severity", "high",
        "--title", "CLI finding", "--finding-file", "src/main.rs", "--lines", "10-12",
        "--category", "correctness", "--confidence", "high",
    ]);
    let findings: serde_json::Value = serde_json::from_str(
        &run_ok(home.path(), &["group", "finding", "list", group, "--json"]),
    ).unwrap();
    let finding = &findings[0];
    for (key, expected) in [
        ("content", "Evidence survives"), ("severity", "high"), ("title", "CLI finding"),
        ("file", "src/main.rs"), ("lines", "10-12"), ("category", "correctness"), ("confidence", "high"),
    ] {
        assert_eq!(finding[key], expected, "{key}");
    }
    let id = finding["id"].as_i64().unwrap().to_string();
    run_ok(home.path(), &["group", "finding", "update", group, &id, "--verdict", "confirmed", "--note", "Reviewed"]);
    let finding: serde_json::Value = serde_json::from_str(
        &run_ok(home.path(), &["group", "finding", "get", group, &id, "--json"]),
    ).unwrap();
    assert_eq!(finding["verdict"], "CONFIRMED");
    assert_eq!(finding["note"], "Reviewed");
    let summary = run_ok(home.path(), &["group", "summary", group]);
    assert!(summary.contains("CLI paths"));
    assert!(summary.contains("Evidence survives"));
    run_ok(home.path(), &["group", "broadcast", group, "Ready to integrate"]);
    let workspace = std::path::PathBuf::from(format!("/tmp/aid-wg-{group}"));
    let broadcast = std::fs::read_to_string(workspace.join("broadcast.md")).unwrap();
    assert!(broadcast.contains("Ready to integrate"));
    std::fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn agent_add_creates_custom_agent_configuration() {
    let home = TempDir::new().unwrap();
    run_ok(home.path(), &["agent", "add", "local-cli-paths"]);
    let config = std::fs::read_to_string(home.path().join("agents/local-cli-paths.toml")).unwrap();
    assert!(config.contains("id = \"local-cli-paths\""));
    assert!(config.contains("[agent]"));
}
