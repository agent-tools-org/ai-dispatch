// Web event endpoint regression for historic unconfirmed acknowledgements.
// Checks raw records, qualified display fields, and frontend field selection.
// Deps: axum extractors, API handler, Store, and embedded JS.
use crate::store::Store;
use crate::types::{EventKind, TaskEvent, TaskId};
use axum::extract::{Path, State};
use std::sync::Arc;

#[tokio::test]
async fn web_legacy_ack_audit_regression() {
    let store = Arc::new(Store::open_memory().unwrap());
    store
        .db()
        .execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at)
        VALUES ('t-web-ack', 'codex', 'task', 'done', ?1)",
            [chrono::Local::now().to_rfc3339()],
        )
        .unwrap();
    store
        .insert_event(&TaskEvent {
            task_id: TaskId("t-web-ack".into()),
            timestamp: chrono::Local::now(),
            event_kind: EventKind::Reasoning,
            detail: "Acked reply".into(),
            metadata: None,
        })
        .unwrap();
    let response = super::api::get_task_events(Path("t-web-ack".into()), State(store))
        .await
        .unwrap();
    let payload = serde_json::to_value(response.0).unwrap();
    assert_eq!(payload[0]["detail"], "Acked reply");
    assert_eq!(payload[0]["legacy_unconfirmed"], true);
    assert!(
        payload[0]["display_detail"]
            .as_str()
            .unwrap()
            .contains("not a confirmed receipt")
    );
    assert!(include_str!("static/app.js").contains("ev.display_detail"));
}
