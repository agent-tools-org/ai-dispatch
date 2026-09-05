// End-to-end coverage for CLI rejection history and dispatch option validation.
// Exercises the real aid binary with isolated state; no agent is launched.
// Deps: common command helpers, tempfile, serde_json, rusqlite.

mod common;
use common::aid_cmd_in;
use serde_json::Value;
use tempfile::TempDir;

fn records(home: &TempDir) -> Vec<Value> {
    std::fs::read_to_string(home.path().join("logs/command-errors.jsonl"))
        .expect("rejection log")
        .lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

#[test]
fn invalid_kind_is_logged_without_prompt_and_suggests_a_supported_kind() {
    let home = TempDir::new().unwrap();
    let output = aid_cmd_in(home.path())
        .args(["run", "grok", "private-prompt-123", "--kind", "audit"])
        .output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--kind debugging"), "{stderr}");
    let events = records(&home);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["stage"], "parse");
    assert_eq!(events[0]["issues"][0]["code"], "InvalidValue");
    assert!(!events[0].to_string().contains("private-prompt-123"));
    assert!(!home.path().join("aid.db").exists());
}

#[test]
fn dispatch_reports_multiple_conflicts_without_creating_a_task() {
    let home = TempDir::new().unwrap();
    let output = aid_cmd_in(home.path()).args([
        "run", "grok", "Audit the scan lifecycle", "--read-only", "--worktree", "fix/scan",
        "--rigor", "critical", "--iterate", "0", "--no-hint",
    ]).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    for text in ["--read-only", "--dir", "--rigor critical", "--iterate"] {
        assert!(stderr.contains(text), "missing {text}: {stderr}");
    }
    let events = records(&home);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["stage"], "validation");
    assert!(events[0]["issues"].as_array().unwrap().len() >= 3);
    let db = rusqlite::Connection::open(home.path().join("aid.db")).unwrap();
    let count: i64 = db.query_row("SELECT count(*) FROM tasks", [], |row| row.get(0)).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn errors_can_be_read_without_initializing_a_database() {
    let home = TempDir::new().unwrap();
    for value in ["audit", "unknown"] {
        let output = aid_cmd_in(home.path()).args(["run", "grok", "brief", "--kind", value])
            .output().unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
    let output = aid_cmd_in(home.path()).args(["errors", "--limit", "1", "--json"])
        .output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let history: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(history.len(), 1);
    assert!(!home.path().join("aid.db").exists());
    assert_eq!(records(&home).len(), 2);
}

#[test]
fn help_and_version_are_not_rejections() {
    let home = TempDir::new().unwrap();
    for args in [vec!["--help"], vec!["run", "--help"], vec!["--version"], vec!["errors", "--json"]] {
        assert!(aid_cmd_in(home.path()).args(args).output().unwrap().status.success());
    }
    assert!(!home.path().join("logs/command-errors.jsonl").exists());
    assert!(!home.path().join("aid.db").exists());
}

#[test]
fn logging_failure_preserves_the_original_parser_error() {
    let home = TempDir::new().unwrap();
    std::fs::write(home.path().join("logs"), "not a directory").unwrap();
    let output = aid_cmd_in(home.path()).args(["run", "grok", "brief", "--kind", "audit"])
        .output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid value"), "{stderr}");
    assert!(stderr.contains("Could not record command error"), "{stderr}");
}

#[test]
fn critical_cannot_be_satisfied_by_disabled_verification() {
    let home = TempDir::new().unwrap();
    for value in ["", "none", "false", "skip"] {
        let output = aid_cmd_in(home.path()).args([
            "run", "grok", "Inspect scan lifecycle", "--rigor", "critical",
            "--verify", value, "--audit", "--no-hint",
        ]).output().unwrap();
        assert!(!output.status.success());
        let events = records(&home);
        let event = events.last().unwrap();
        assert_eq!(event["stage"], "validation");
        assert_eq!(event["issues"][0]["code"], "critical_proof_required");
    }
}

#[test]
fn concurrent_parse_errors_produce_complete_redacted_records() {
    let home = TempDir::new().unwrap();
    let mut children = Vec::new();
    for _ in 0..12 {
        children.push(aid_cmd_in(home.path())
            .args(["run", "grok", "private-prompt-456", "--kind=private-value-789"])
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
            .spawn().unwrap());
    }
    for mut child in children { assert_eq!(child.wait().unwrap().code(), Some(2)); }
    let events = records(&home);
    assert_eq!(events.len(), 12);
    for event in events {
        let text = event.to_string();
        assert!(!text.contains("private-prompt-456"));
        assert!(!text.contains("private-value-789"));
    }
}

#[test]
fn unstructured_command_errors_are_marked_without_copying_their_raw_details() {
    let home = TempDir::new().unwrap();
    let output = aid_cmd_in(home.path()).args(["show", "t-00000000", "--events"])
        .output().unwrap();
    assert!(!output.status.success());
    let events = records(&home);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["stage"], "command");
    assert_eq!(events[0]["issues"][0]["code"], "CommandFailed");
    assert!(!events[0].to_string().contains("t-00000000"));
}
