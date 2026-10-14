// Binary regressions for Codex interactive-input refusals.
// Verifies steer/reply/respond leave no queued messages or signal files.
// Deps: compiled aid binary, rusqlite, tempfile, and shared command helpers.

mod common;
use common::aid_cmd_in;
use rusqlite::Connection;

#[test]
fn codex_steer_reply_respond_refuse_without_side_effects() {
    let home = tempfile::tempdir().unwrap();
    let initialized = aid_cmd_in(home.path())
        .args(["board", "--json"])
        .output()
        .unwrap();
    assert!(initialized.status.success());
    let db = Connection::open(home.path().join("aid.db")).unwrap();
    db.execute(
        "INSERT INTO tasks (id, agent, prompt, status, created_at)
         VALUES ('t-codex-input', 'codex', 'prompt', 'running', ?1)",
        [chrono::Local::now().to_rfc3339()],
    )
    .unwrap();
    for command in ["steer", "reply", "respond"] {
        let output = aid_cmd_in(home.path())
            .args([command, "t-codex-input", "pivot"])
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "{command} must refuse Codex exec input"
        );
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("aid retry t-codex-input -f"), "{error}");
        let count: i64 = db
            .query_row("SELECT count(*) FROM task_messages", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
        assert!(!home.path().join("jobs/t-codex-input.steer").exists());
        assert!(!home.path().join("jobs/t-codex-input.input").exists());
    }
}
