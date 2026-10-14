// Binary regressions for unstick queueing and unconfirmed reply timeouts.
// Checks the CLI label against the persisted pending message.
// Deps: compiled aid, isolated HOME, SQLite, and shared command helpers.
mod common;
use common::aid_cmd_in;
use rusqlite::Connection;

#[test]
fn manual_unstick_queue_status_audit_regression() {
    let home = tempfile::tempdir().unwrap();
    assert!(
        aid_cmd_in(home.path())
            .args(["board", "--json"])
            .output()
            .unwrap()
            .status
            .success()
    );
    let db = Connection::open(home.path().join("aid.db")).unwrap();
    db.execute(
        "INSERT INTO tasks (id, agent, prompt, status, created_at)
        VALUES ('t-queue', 'gemini', 'task', 'running', ?1)",
        [chrono::Local::now().to_rfc3339()],
    )
    .unwrap();
    std::fs::create_dir_all(home.path().join("jobs")).unwrap();
    std::fs::write(
        home.path().join("jobs/t-queue.json"),
        serde_json::json!({
            "task_id": "t-queue", "worker_pid": std::process::id(), "agent_name": "gemini",
            "prompt": "task", "dir": null, "output": null, "model": null,
            "verify": null, "retry": 0, "group": null
        })
        .to_string(),
    )
    .unwrap();
    let output = aid_cmd_in(home.path())
        .args(["unstick", "t-queue", "--message", "pivot"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Queued unstick nudge"), "{text}");
    let pending: i64 = db
        .query_row(
            "SELECT count(*) FROM task_messages
        WHERE delivered_at IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(pending, 1);
}

#[test]
fn reply_timeout_is_unconfirmed_audit_regression() {
    let home = tempfile::tempdir().unwrap();
    assert!(
        aid_cmd_in(home.path())
            .args(["board", "--json"])
            .output()
            .unwrap()
            .status
            .success()
    );
    let db = Connection::open(home.path().join("aid.db")).unwrap();
    db.execute(
        "INSERT INTO tasks (id, agent, prompt, status, created_at)
        VALUES ('t-timeout', 'gemini', 'task', 'running', ?1)",
        [chrono::Local::now().to_rfc3339()],
    )
    .unwrap();
    let output = aid_cmd_in(home.path())
        .args(["reply", "t-timeout", "pivot", "--timeout", "0"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("PTY write unconfirmed"), "{text}");
    assert!(!text.contains("delivered:"), "{text}");
    let pending: i64 = db
        .query_row(
            "SELECT count(*) FROM task_messages
        WHERE delivered_at IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(pending, 1);
}
