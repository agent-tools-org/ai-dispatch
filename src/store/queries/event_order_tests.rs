// Event read equivalence for nonmonotonic timestamps, ties, and corrupt unselected rows.
// Verifies the conservative full-history behavior retained for wait and liveness.

use super::*;

fn store() -> Store {
    let store = Store::open_memory().expect("store");
    store
        .db()
        .execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at)
         VALUES ('t-order', 'codex', 'prompt', 'running', '2026-10-23T10:00:00Z')",
            [],
        )
        .expect("task");
    store
}

#[test]
fn event_selectors_match_history_when_timestamp_order_differs_from_id_order() {
    let store = store();
    for (timestamp, detail) in [
        ("2026-10-23T12:00:00Z", "later"),
        ("2026-10-23T11:00:00Z", "earlier"),
        ("2026-10-23T12:00:00Z", "tie"),
    ] {
        store
            .db()
            .execute(
                "INSERT INTO events (task_id, timestamp, event_type, detail, metadata)
             VALUES ('t-order', ?1, 'error', ?2, ?3)",
                params![
                    timestamp,
                    detail,
                    serde_json::json!({"awaiting_prompt": detail, "verify_execution_error": true})
                        .to_string()
                ],
            )
            .expect("event");
        let expected = store
            .get_events("t-order")
            .expect("history")
            .last()
            .cloned()
            .expect("last");
        assert_eq!(
            store.latest_waiting_prompt("t-order").expect("prompt"),
            Some(expected.detail.clone())
        );
        assert_eq!(
            store.latest_infrastructure_error("t-order").expect("infra"),
            Some(expected.detail.clone())
        );
        assert_eq!(
            store.liveness_summary("t-order").expect("activity").1,
            Some((expected.timestamp, expected.detail))
        );
    }
}

#[test]
fn wait_and_liveness_keep_main_errors_for_corrupt_setup_events() {
    let store = store();
    store
        .db()
        .execute(
            "INSERT INTO events (task_id, timestamp, event_type, detail)
         VALUES ('t-order', '2026-10-23T12:00:00Z', 'setup', X'ff')",
            [],
        )
        .expect("corrupt setup");
    assert!(store.get_events("t-order").is_err());
    assert!(store.latest_waiting_prompt("t-order").is_err());
    assert!(store.liveness_summary("t-order").is_err());
}
