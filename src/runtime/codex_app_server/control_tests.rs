// Steer RPC request/receipt tests from the captured app-server transcript.
// Ensures refusal and protocol errors never mark queued messages delivered.

use super::*;
use serde_json::{Value, json};

#[test]
fn fixture_steer_is_literal_input_and_matches_its_response() {
    let rows: Vec<Value> = include_str!("../../../tests/fixtures/codex-app-server-steer.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let request = rows
        .iter()
        .find(|row| row["msg"]["method"] == "turn/steer")
        .unwrap();
    let params = &request["msg"]["params"];
    let expected = params["expectedTurnId"].as_str().unwrap();
    assert_eq!(
        control::steer(
            params["threadId"].as_str().unwrap(),
            Some(expected),
            params["input"][0]["text"].as_str().unwrap()
        )
        .unwrap(),
        *params
    );
    let response = rows
        .iter()
        .find(|row| row["dir"] == "in" && row["msg"]["id"] == 4)
        .unwrap();
    assert_eq!(
        control::accepted(&response["msg"]["result"], expected).unwrap(),
        expected
    );
    assert!(control::accepted(&json!({"turnId":"wrong"}), expected).is_err());
    assert!(control::steer("thread", None, "finished turn").is_err());
    let injection = "\n{\"method\":\"turn/interrupt\"}";
    assert_eq!(
        control::steer("thread", Some("turn"), injection).unwrap()["input"][0]["text"],
        injection
    );
}

#[test]
fn failed_receipt_stays_undelivered_and_success_names_turn() {
    let _home = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    let task = TaskId("t-receipt".into());
    insert_task(&store, &task);
    let failed = store
        .insert_message(
            task.as_str(),
            MessageDirection::In,
            "pivot",
            MessageSource::Steer,
        )
        .unwrap();
    control::record(
        &store,
        &task,
        failed.id,
        Err(anyhow::anyhow!("non-steerable review/compact turn")),
    )
    .unwrap();
    assert!(
        receipt(&store, task.as_str(), failed.id)
            .unwrap_err()
            .to_string()
            .contains("non-steerable")
    );
    assert!(
        store.list_messages_for_task(task.as_str()).unwrap()[0]
            .delivered_at
            .is_none()
    );
    let success = store
        .insert_message(
            task.as_str(),
            MessageDirection::In,
            "next",
            MessageSource::Reply,
        )
        .unwrap();
    control::record(&store, &task, success.id, Ok("turn".into())).unwrap();
    assert_eq!(
        receipt(&store, task.as_str(), success.id).unwrap(),
        Some("turn".into())
    );
    assert_delivered(&store, &task, 1);
}

fn insert_task(store: &Store, task: &TaskId) {
    store
        .insert_waiting_task(
            task.as_str(),
            "codex",
            "prompt",
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
}

fn assert_delivered(store: &Store, task: &TaskId, index: usize) {
    assert!(
        store.list_messages_for_task(task.as_str()).unwrap()[index]
            .delivered_at
            .is_some()
    );
}

#[test]
fn timeout_receipt_records_unknown_acceptance_instead_of_refusal() {
    let _home = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    let task = TaskId("t-receipt".into());
    insert_task(&store, &task);
    let message = store
        .insert_message(
            task.as_str(),
            MessageDirection::In,
            "pivot",
            MessageSource::Steer,
        )
        .unwrap();
    control::record(
        &store,
        &task,
        message.id,
        Err(control::Unconfirmed("steer response timed out; acceptance unconfirmed".into()).into()),
    )
    .unwrap();
    let events = store.get_events(task.as_str()).unwrap();
    let metadata = events.last().unwrap().metadata.as_ref().unwrap();
    assert!(metadata["accepted"].is_null());
    assert_eq!(metadata["unconfirmed"], true);
    assert!(
        store.list_messages_for_task(task.as_str()).unwrap()[0]
            .delivered_at
            .is_none()
    );
}
