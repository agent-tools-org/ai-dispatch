// Surface regressions for historic output-derived acknowledgements.
// Checks show text/JSON and MCP without changing persisted event records.
// Deps: show, MCP tools, Store, and task event types.
use super::*;
use crate::types::{EventKind, TaskEvent, TaskId};
use serde_json::json;

fn fixture() -> Arc<Store> {
    let store = Arc::new(Store::open_memory().unwrap());
    store
        .db()
        .execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at)
        VALUES ('t-legacy-ack', 'codex', 'task', 'done', ?1)",
            [chrono::Local::now().to_rfc3339()],
        )
        .unwrap();
    store
        .insert_event(&TaskEvent {
            task_id: TaskId("t-legacy-ack".into()),
            timestamp: chrono::Local::now(),
            event_kind: EventKind::Reasoning,
            detail: "Acked reply".into(),
            metadata: Some(json!({"acked_reply": true})),
        })
        .unwrap();
    store
}

#[test]
fn show_text_legacy_ack_audit_regression() {
    let store = fixture();
    for full in [false, true] {
        let text = text::events_text(&store, "t-legacy-ack", full).unwrap();
        assert!(text.contains("not a confirmed receipt"), "{text}");
    }
    let audit = render_mode_text(&store, "t-legacy-ack", ShowMode::Default).unwrap();
    assert!(audit.contains("not a confirmed receipt"), "{audit}");
    let event = store.get_events("t-legacy-ack").unwrap().remove(0);
    assert_eq!(event.detail, "Acked reply");
    assert_eq!(event.metadata, Some(json!({"acked_reply": true})));
}

#[test]
fn show_json_legacy_ack_audit_regression() {
    let payload: serde_json::Value =
        serde_json::from_str(&show_json::task_json(&fixture(), "t-legacy-ack").unwrap()).unwrap();
    let event = &payload["events"][0];
    assert_eq!(event["detail"], "Acked reply");
    assert_eq!(event["legacy_unconfirmed"], true);
    assert!(
        event["display_detail"]
            .as_str()
            .unwrap()
            .contains("not a confirmed receipt")
    );
}

#[tokio::test]
async fn mcp_legacy_ack_audit_regression() {
    let result = crate::cmd::mcp_tools::call_tool(
        fixture(),
        "aid_show",
        json!({"task_id": "t-legacy-ack", "mode": "events"}),
    )
    .await
    .unwrap();
    let body: serde_json::Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(
        body["content"]
            .as_str()
            .unwrap()
            .contains("not a confirmed receipt")
    );
}
