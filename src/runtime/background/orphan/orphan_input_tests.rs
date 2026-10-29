// Reaper liveness tests for aid-authored input bookkeeping: steers, replies and
// failed deliveries are written by aid, not the agent, so they are not progress.
// Deps: orphan latest_activity, parent test fixtures and the store.

use super::super::latest_activity;
use super::make_task;
use crate::store::Store;
use crate::types::{EventKind, TaskEvent};
use chrono::{Duration, Local};
use serde_json::{Value, json};

#[test]
fn operator_input_bookkeeping_does_not_postpone_the_reaper() {
    let _temp = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().expect("store");
    let task = make_task("t-input-bookkeeping");
    store.insert_task(&task).expect("insert task");
    let now = Local::now();
    let event =
        |age: i64, event_kind: EventKind, detail: &str, metadata: Option<Value>| TaskEvent::at(
            task.id.clone(),
            event_kind,
            detail.to_string(),
            now - Duration::seconds(age),
        ).with_metadata(metadata);
    let agent_event = event(600, EventKind::Reasoning, "agent progress", None);
    let bookkeeping = [
        event(
            30,
            EventKind::Reasoning,
            "Steered: go",
            Some(json!({ "steered": true, "delivered": true })),
        ),
        event(
            20,
            EventKind::Reasoning,
            "Replied: ok",
            Some(json!({ "message_id": 7, "source": "cli" })),
        ),
        event(
            10,
            EventKind::Error,
            "Steer not delivered: go",
            Some(json!({
                "input_delivery": "failed", "delivered": false, "source": "Steer", "message_id": null,
            })),
        ),
    ];
    for event in std::iter::once(&agent_event).chain(&bookkeeping) {
        store.insert_event(event).expect("insert event");
    }

    let activity = latest_activity(&store, &task).expect("activity");

    assert_eq!(activity.event_count, 1);
    assert_eq!(activity.detail.as_deref(), Some("agent progress"));
    assert_eq!(
        activity.timestamp.timestamp(),
        agent_event.timestamp.timestamp()
    );
}
