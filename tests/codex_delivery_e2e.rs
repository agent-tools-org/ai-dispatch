// E2E coverage for Codex successful exits that omit the final deliverable.
// Verifies delivery failure persistence and one-shot same-session recovery.
// Deps: compiled aid binary, a PATH-injected fake Codex CLI, and SQLite.

use rusqlite::Connection;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tempfile::TempDir;

mod common;
use common::aid_cmd_in;

#[test]
fn fresh_codex_accepts_the_current_approval_flag() {
    assert_fresh_codex_succeeds("t-current-codex-approval", false);
}

#[test]
fn fresh_codex_accepts_the_legacy_approval_flag() {
    assert_fresh_codex_succeeds("t-legacy-codex-approval", true);
}

#[test]
fn exact_short_answer_is_a_successful_delivery() {
    let aid_home = TempDir::new().unwrap();
    let bin_dir = TempDir::new().unwrap();
    write_fake_codex(bin_dir.path());

    let output = aid_cmd_in(aid_home.path())
        .env("PATH", test_path(bin_dir.path()))
        .env("FAKE_CODEX_SHORT_FINAL", "1")
        .args([
            "run",
            "codex",
            "Reply with the single word: ok",
            "--id",
            "t-short-final",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let conn = Connection::open(aid_home.path().join("aid.db")).unwrap();
    assert_eq!(task_facts(&conn, "t-short-final").0, "done");
}

#[test]
fn missing_explicit_result_file_fails_on_the_artifact_contract() {
    let aid_home = TempDir::new().unwrap();
    let bin_dir = TempDir::new().unwrap();
    write_fake_codex(bin_dir.path());

    let output = aid_cmd_in(aid_home.path())
        .env("PATH", test_path(bin_dir.path()))
        .env("FAKE_CODEX_SHORT_FINAL", "1")
        .args([
            "run",
            "codex",
            "Reply with the single word: ok",
            "--result-file",
            "required-result.md",
            "--id",
            "t-missing-required-result",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let conn = Connection::open(aid_home.path().join("aid.db")).unwrap();
    let facts = task_facts(&conn, "t-missing-required-result");
    assert_eq!(facts.0, "failed");
    assert_eq!(facts.1.as_deref(), Some("missing_final_delivery"));
}

fn assert_fresh_codex_succeeds(task_id: &str, legacy: bool) {
    let aid_home = TempDir::new().unwrap();
    let bin_dir = TempDir::new().unwrap();
    write_fake_codex(bin_dir.path());

    let mut command = aid_cmd_in(aid_home.path());
    command
        .env("PATH", test_path(bin_dir.path()))
        .env("FAKE_CODEX_FINAL_ON_FRESH", "1");
    if legacy {
        command.env("FAKE_CODEX_LEGACY", "1");
    }
    let output = command
        .args(["run", "codex", "Implement and report the requested change", "--id", task_id])
        .output().unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let conn = Connection::open(aid_home.path().join("aid.db")).unwrap();
    assert_eq!(task_facts(&conn, task_id).0, "done");
}

#[test]
fn missing_final_delivery_fails_parent_and_resumes_once() {
    let aid_home = TempDir::new().unwrap();
    let bin_dir = TempDir::new().unwrap();
    write_fake_codex(bin_dir.path());
    let path = format!(
        "{}:{}",
        bin_dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let home = home_with_seeded_rollout();
    let output = aid_cmd_in(aid_home.path())
        .env("PATH", path)
        .env("HOME", home.path())
        .args([
            "run",
            "codex",
            "Investigate and report the result",
            "--read-only",
            "--id",
            "t-delivery-parent",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let conn = Connection::open(aid_home.path().join("aid.db")).unwrap();
    let parent = task_facts(&conn, "t-delivery-parent");
    assert_eq!(parent.0, "failed");
    assert_eq!(parent.1.as_deref(), Some("missing_final_delivery"));
    let child = child_facts(&conn, "t-delivery-parent");
    assert_eq!(child.0, "done");
    assert_eq!(child.1, 1);
}

#[test]
fn repeated_missing_delivery_does_not_start_third_attempt() {
    let aid_home = TempDir::new().unwrap();
    let bin_dir = TempDir::new().unwrap();
    write_fake_codex(bin_dir.path());

    let home = home_with_seeded_rollout();
    let output = aid_cmd_in(aid_home.path())
        .env("PATH", test_path(bin_dir.path()))
        .env("HOME", home.path())
        .env("FAKE_CODEX_HOLLOW_RESUME", "1")
        .args([
            "run",
            "codex",
            "Investigate and report the result",
            "--read-only",
            "--id",
            "t-delivery-repeat",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success(), "repeated hollow delivery must fail");

    let conn = Connection::open(aid_home.path().join("aid.db")).unwrap();
    let facts: (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), SUM(status = 'failed') FROM tasks",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(facts, (2, 2));
}

/// The resume preflight looks for the session's rollout under the operator's
/// real `$HOME`, so a test that leaves HOME alone reads whatever that machine
/// happens to have and falls back to a fresh run instead of resuming. Give the
/// test its own HOME and seed the rollout the fake CLI's thread_id implies.
const FIXTURE_SESSION_ID: &str = "019e3e49-6b83-7563-a3d8-b51a3a716dd1";

fn home_with_seeded_rollout() -> TempDir {
    let home = TempDir::new().unwrap();
    let sessions = home.path().join(".codex/sessions/2026/08/09");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(
        sessions.join(format!("rollout-2026-08-09T00-00-00-{FIXTURE_SESSION_ID}.jsonl")),
        "{\"type\":\"turn_context\",\"model\":\"gpt-5.6-sol\"}\n",
    )
    .unwrap();
    home
}

fn task_facts(conn: &Connection, task_id: &str) -> (String, Option<String>) {
    conn.query_row(
        "SELECT status, delivery_assessment FROM tasks WHERE id = ?1",
        [task_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap()
}

fn child_facts(conn: &Connection, parent_id: &str) -> (String, i64) {
    conn.query_row(
        "SELECT status, COUNT(*) OVER () FROM tasks WHERE parent_task_id = ?1",
        [parent_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap()
}

fn write_fake_codex(dir: &Path) {
    let path = dir.join("codex");
    std::fs::write(
        &path,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  if [ "$FAKE_CODEX_LEGACY" = "1" ]; then
    echo "codex-cli 0.146.0"
  else
    echo "codex-cli 0.147.0"
  fi
  exit 0
fi
if [ "$1" = "exec" ] && [ "$2" = "--help" ]; then
  if [ "$FAKE_CODEX_LEGACY" = "1" ]; then
    echo "      --full-auto"
  else
    echo "      --approve-for-me"
  fi
  exit 0
fi
if [ "$2" = "resume" ]; then
  printf '%s\n' '{"type":"thread.started","thread_id":"019e3e49-6b83-7563-a3d8-b51a3a716dd1"}'
  if [ "$FAKE_CODEX_HOLLOW_RESUME" = "1" ]; then
    printf '%s\n' '{"type":"item.completed","item":{"id":"work-again","type":"command_execution","command":"inspect again","aggregated_output":"","exit_code":0}}'
    printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":20,"cached_input_tokens":10,"output_tokens":8000}}'
    exit 0
  fi
  printf '%s\n' '{"type":"item.completed","item":{"id":"final","type":"agent_message","text":"Recovery complete."}}'
  printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":20,"cached_input_tokens":10,"output_tokens":80}}'
  exit 0
fi
if [ "$FAKE_CODEX_LEGACY" = "1" ]; then
  case " $* " in
    *" --full-auto "*) ;;
    *) echo "error: missing '--full-auto'" >&2; exit 2 ;;
  esac
else
  case " $* " in
    *" --approve-for-me "*) ;;
    *) echo "error: missing '--approve-for-me'" >&2; exit 2 ;;
  esac
fi
printf '%s\n' '{"type":"thread.started","thread_id":"019e3e49-6b83-7563-a3d8-b51a3a716dd1"}'
if [ "$FAKE_CODEX_SHORT_FINAL" = "1" ]; then
  printf '%s\n' '{"type":"item.completed","item":{"id":"final","type":"agent_message","text":"ok"}}'
  printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":20,"cached_input_tokens":10,"output_tokens":1}}'
  exit 0
fi
if [ "$FAKE_CODEX_FINAL_ON_FRESH" = "1" ]; then
  printf '%s\n' '{"type":"item.completed","item":{"id":"final","type":"agent_message","text":"Completed and verified."}}'
  printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":20,"cached_input_tokens":10,"output_tokens":80}}'
  exit 0
fi
printf '%s\n' '{"type":"item.completed","item":{"id":"progress","type":"agent_message","text":"I am investigating the task now. I will inspect the available evidence and provide the final report after the tool work is complete. This progress update is deliberately substantive but is not a final answer."}}'
printf '%s\n' '{"type":"item.completed","item":{"id":"work","type":"command_execution","command":"inspect evidence","aggregated_output":"","exit_code":0}}'
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":100,"cached_input_tokens":50,"output_tokens":9000}}'
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).unwrap();
}

fn test_path(bin_dir: &Path) -> String {
    format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}
