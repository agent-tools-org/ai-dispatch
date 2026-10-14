// Binary regression for migrated event history in show text and JSON.
// Uses an isolated seeded SQLite store and the compiled aid executable.
mod common;
use common::aid_cmd_in;

#[test]
fn show_migrates_legacy_receipt_claims_at_rest() {
    let home = tempfile::tempdir().unwrap();
    assert!(aid_cmd_in(home.path()).arg("board").output().unwrap().status.success());
    let db = rusqlite::Connection::open(home.path().join("aid.db")).unwrap();
    db.execute(
        "INSERT INTO tasks (id, agent, prompt, status, created_at)
         VALUES ('t-old', 'gemini', 'task', 'done', ?1)",
        [chrono::Local::now().to_rfc3339()],
    ).unwrap();
    for (detail, metadata) in [
        ("Acked reply", r#"{"acked_reply":true}"#),
        ("Steered: pivot", r#"{"steered":true}"#),
        ("Hard-cap wrap-up: committing work in progress and writing the report requested",
         r#"{"steered":true,"hard_cap_wrap_up":true}"#),
    ] {
        db.execute(
            "INSERT INTO events (task_id, timestamp, event_type, detail, metadata)
             VALUES ('t-old', ?1, 'reasoning', ?2, ?3)",
            rusqlite::params![chrono::Local::now().to_rfc3339(), detail, metadata],
        ).unwrap();
    }
    for args in [vec!["show", "t-old"], vec!["show", "t-old", "--json"]] {
        let output = aid_cmd_in(home.path()).args(args).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains("Acked reply"), "{text}");
        assert!(!text.contains("\"steered\":true"), "{text}");
        assert!(!text.contains("\"steered\": true"), "{text}");
        assert!(text.contains("consumption unconfirmed"), "{text}");
        assert!(text.contains("queued request"), "{text}");
    }
}
