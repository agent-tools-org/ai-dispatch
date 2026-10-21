// Typed rbox receipts drive verify status even when command output resembles rbox errors.
// Uses a fake rbox on PATH and the shell settlement agent; no live box contact.
#![cfg(unix)]
#[allow(dead_code)]
mod common;
#[allow(dead_code)]
#[path = "common/settlement.rs"]
mod settlement;

use settlement::Harness;

const FAKE_RBOX: &str = r#"#!/bin/bash
if [[ $1 == exec && $2 == --help ]]; then
  [[ $LEGACY == 1 ]] || echo '--result-file PATH'
  exit 0
fi
result=''
while [[ $# -gt 0 ]]; do
  if [[ $1 == --result-file ]]; then result=$2; shift; fi
  shift
done
if [[ -n $result && $NO_RESULT != 1 ]]; then
  printf '%s\n' "$RECEIPT" > "$result"
fi
[[ -z $DIAGNOSTIC ]] || printf '%s\n' "$DIAGNOSTIC" >&2
exit "$COMMAND_STATUS"
"#;

fn run_case(phase: &str, reason: &str, code: i32, diagnostic: &str, legacy: bool, missing: bool) {
    let h = Harness::with_git_project();
    let bin = h.project().join("bin");
    std::fs::create_dir(&bin).expect("bin");
    common::write_executable(&bin.join("rbox"), FAKE_RBOX);
    let receipt = serde_json::json!({
        "version": 1, "phase": phase, "job": null, "command_exit": code,
        "reason": reason, "detail": "typed admission detail"
    });
    let output = h
        .aid()
        .env(
            "PATH",
            format!("{}:{}", bin.display(), std::env::var("PATH").expect("PATH")),
        )
        .env("RECEIPT", receipt.to_string())
        .env("LEGACY", if legacy { "1" } else { "0" })
        .env("NO_RESULT", if missing { "1" } else { "0" })
        .env("DIAGNOSTIC", diagnostic)
        .env("COMMAND_STATUS", code.to_string())
        .args([
            "run",
            settlement::AGENT,
            "settle",
            "--id",
            "t-typed-rbox",
            "--dir",
        ])
        .arg(h.project())
        .args(["--remote-build", "fake", "--verify", "cargo test"])
        .output()
        .expect("run");
    let infrastructure = if legacy || missing {
        !diagnostic.is_empty()
    } else {
        phase == "not_started"
    };
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let detail = if legacy || missing {
        diagnostic
    } else {
        "typed admission detail"
    };
    assert_result(&h, infrastructure, detail);
}

fn assert_result(h: &Harness, infrastructure: bool, detail: &str) {
    let db = rusqlite::Connection::open(h.home().join("aid.db")).expect("db");
    let status: String = db
        .query_row(
            "SELECT verify_status FROM tasks WHERE id='t-typed-rbox'",
            [],
            |row| row.get(0),
        )
        .expect("verify status");
    assert_eq!(
        status,
        if infrastructure {
            "infrastructure_failure"
        } else {
            "failed"
        }
    );
    assert_eq!(
        h.status("t-typed-rbox").as_deref(),
        Some(if infrastructure { "done" } else { "failed" })
    );
    if infrastructure {
        let board = h.aid().args(["board"]).output().expect("board");
        let board = String::from_utf8_lossy(&board.stdout);
        assert!(board.contains("[VINFRA]"), "{board}");
        assert!(board.contains(detail), "{board}");
    }
}

#[test]
fn typed_not_started_without_stderr_is_infrastructure() {
    run_case("not_started", "lock_timeout", 1, "", false, false);
}

#[test]
fn typed_completed_numeric_exits_override_rbox_looking_stderr() {
    for code in [75, 69, 124] {
        run_case(
            "completed",
            "completed",
            code,
            "rbox: Box lock not acquired within 60 s",
            false,
            false,
        );
    }
}

#[test]
fn older_rbox_keeps_legacy_text_classification() {
    run_case(
        "completed",
        "completed",
        75,
        "rbox: Box lock not acquired within 60 s",
        true,
        false,
    );
}

#[test]
fn missing_receipt_keeps_legacy_text_classification() {
    run_case(
        "completed",
        "completed",
        75,
        "rbox: Box lock not acquired within 60 s",
        false,
        true,
    );
}

#[test]
fn unknown_refusal_reason_still_records_typed_detail() {
    run_case("not_started", "future_reason", 1, "", false, false);
}
