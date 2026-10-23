// End-to-end clean retention reporting and historical event rendering.
// Runs the real CLI with a disposable home and never launches an agent.
mod common;
use common::aid_cmd_in;
use rusqlite::Connection;
use serde_json::json;
use tempfile::TempDir;

fn seeded_home() -> TempDir {
    let home = TempDir::new().unwrap();
    let output = aid_cmd_in(home.path())
        .args(["clean", "--dry-run"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let conn = Connection::open(home.path().join("aid.db")).unwrap();
    conn.execute_batch(
        "INSERT INTO tasks (id,agent,prompt,status,created_at,completed_at)
         VALUES ('t-retention','codex','test','done','2020-01-01','2020-01-01');",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO events (task_id,timestamp,event_type,detail,metadata)
         VALUES ('t-retention','2020-01-01T00:00:00Z','build','build output',?1)",
        [json!({"full": "x".repeat(5000), "command": "build", "exit_code": 0}).to_string()],
    )
    .unwrap();
    home
}

#[test]
fn clean_dry_run_reports_rows_bytes_and_preserves_payload() {
    let home = seeded_home();
    let output = aid_cmd_in(home.path())
        .args(["clean", "--dry-run", "--vacuum"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("Would trim 1 event rows (5000 payload bytes removed)"),
        "{text}"
    );
    assert!(text.contains("Would VACUUM"), "{text}");
    let conn = Connection::open(home.path().join("aid.db")).unwrap();
    let full: String = conn
        .query_row(
            "SELECT json_extract(metadata,'$.full') FROM events",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(full, "x".repeat(5000));
}

#[test]
fn clean_then_show_export_and_repeat_explain_trimmed_output() {
    let home = seeded_home();
    let output = aid_cmd_in(home.path()).arg("clean").output().unwrap();
    assert!(output.status.success(), "{output:?}");
    for args in [
        vec!["show", "t-retention", "--events"],
        vec!["show", "t-retention", "--events", "--full"],
        vec!["export", "t-retention"],
        vec!["export", "t-retention", "--format", "json"],
    ] {
        let output = aid_cmd_in(home.path()).args(args).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("output trimmed"),
            "{output:?}"
        );
    }
    let repeat = aid_cmd_in(home.path()).arg("clean").output().unwrap();
    assert!(repeat.status.success(), "{repeat:?}");
    assert!(String::from_utf8_lossy(&repeat.stdout).contains("Trimmed 0 event rows"));
}
