// Regression coverage for truthful persisted input history.
// Seeds historical records, reopens the store, and checks explain context and idempotence.
use crate::store::Store;
use serde_json::{json, Value};

#[test]
fn legacy_receipts_are_rewritten_once_and_explain_uses_truthful_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("aid.db");
    let store = Store::open(&path).unwrap();
    store.db().execute(
        "INSERT INTO tasks (id, agent, prompt, status, created_at)
         VALUES ('t-old', 'gemini', 'task', 'done', ?1)",
        [chrono::Local::now().to_rfc3339()],
    ).unwrap();
    let cases = [
        ("Acked reply", json!({}), "Response written to PTY; consumption unconfirmed", "written"),
        ("old acknowledgement", json!({"acked_reply": true, "full": "Acked reply"}),
         "Response written to PTY; consumption unconfirmed", "written"),
        ("Steered: pivot", json!({"steered": true, "delivered": true}),
         "Steer written to PTY; consumption unconfirmed: pivot", "written"),
        ("Hard-cap wrap-up: committing work in progress and writing the report requested",
         json!({"steered": true, "hard_cap_wrap_up": true}),
         "Hard-cap wrap-up: queued request to commit work in progress and write the report", "queued"),
        ("Replied: yes", json!({"message_id": 12, "delivered": true}),
         "Reply written to PTY; consumption unconfirmed: yes", "written"),
    ];
    for (detail, metadata, _, _) in &cases {
        store.db().execute(
            "INSERT INTO events (task_id, timestamp, event_type, detail, metadata)
             VALUES ('t-old', ?1, 'reasoning', ?2, ?3)",
            rusqlite::params![chrono::Local::now().to_rfc3339(), detail, metadata.to_string()],
        ).unwrap();
    }
    drop(store);
    let store = Store::open(&path).unwrap();
    let events = store.get_events("t-old").unwrap();
    assert_eq!(events.len(), cases.len());
    assert_rewritten_events(&events, &cases);
    assert_eq!(events[2].metadata.as_ref().unwrap()["source"], "steer");
    assert_eq!(events[3].metadata.as_ref().unwrap()["hard_cap_wrap_up"], true);
    let task = store.get_task("t-old").unwrap().unwrap();
    let context = crate::cmd::explain::build_explain_context(&task, &events, "", "");
    assert!(!context.contains("Acked reply"), "{context}");
    assert!(!context.contains("\"steered\":true"), "{context}");
    let snapshot: Value = serde_json::to_value(&events).unwrap();
    drop(store);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(serde_json::to_value(reopened.get_events("t-old").unwrap()).unwrap(), snapshot);
    reopened.db().prepare("SELECT acked_at FROM task_messages").unwrap();
}

fn assert_rewritten_events(
    events: &[crate::types::TaskEvent],
    cases: &[(&str, Value, &str, &str)],
) {
    for (event, (_, _, expected, stage)) in events.iter().zip(cases) {
        assert_eq!(&event.detail, expected);
        assert_eq!(event.full_detail(), *expected);
        let meta = event.metadata.as_ref().unwrap();
        assert_eq!(meta["legacy_rewritten"], true);
        assert_eq!(meta["input_delivery"], *stage);
        for key in ["acked_reply", "steered", "delivered"] {
            assert!(meta.get(key).is_none(), "{meta}");
        }
        if *stage == "written" {
            assert_eq!(meta["consumption"], "unconfirmed");
        }
    }
}
