// Receipt regressions distinguish terminal task rows from settled workers.
// Tests legacy evidence, persisted settlement and model isolation without agents.
use super::*;
use crate::{support::paths, types::*};

fn setup() -> (tempfile::TempDir, paths::AidHomeGuard, Store) {
    let home = tempfile::tempdir().expect("home");
    let guard = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("dirs");
    let store = Store::open_memory().expect("store");
    (home, guard, store)
}

#[test]
fn caller_owned_receipt_terminal_is_not_worker_settlement() {
    let (_home, _guard, store) = setup();
    let mut task = crate::cost::test_support::task(AgentKind::Codex, 0, None);
    task.id = TaskId("t-receipt".into());
    task.status = TaskStatus::Failed;
    task.requested_model = Some("own-request".into());
    task.observed_model = Some("own-observed".into());
    store.insert_task(&task).expect("task");
    let before = receipt(&store, task.id.as_str()).expect("receipt");
    assert_eq!(before["task"]["terminal"], true);
    assert_eq!(before["settled"], false);
    assert_eq!(before["task"]["requested_model"], "own-request");
    assert_eq!(before["task"]["observed_model"], "own-observed");
    store
        .settle_worker(task.id.as_str(), "completed", None)
        .expect("settle");
    let after = receipt(&store, task.id.as_str()).expect("receipt");
    assert_eq!(after["worker"]["state"], "settled");
    assert_eq!(after["worker"]["outcome"], "completed");
    assert_eq!(after["worker"]["pid"], Value::Null);
    assert_eq!(after["settled"], true);
    assert!(after["revision"].as_i64() > before["revision"].as_i64());
    assert_eq!(
        receipt(&store, task.id.as_str()).expect("repeat")["revision"],
        after["revision"]
    );
}

#[test]
fn caller_owned_receipt_legacy_rows_are_unconfirmed_and_missing_ids_are_structured() {
    let (_home, _guard, store) = setup();
    store
        .db()
        .execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at)
        VALUES ('t-legacy', 'codex', 'prompt', 'done', '2026-10-29T10:00:00Z')",
            [],
        )
        .expect("legacy row");
    let receipt = receipt(&store, "t-legacy").expect("receipt");
    assert_eq!(receipt["worker"]["state"], "unconfirmed");
    assert_eq!(receipt["settled"], false);
    assert!(receipt["policy"].is_null());
    let error = super::receipt(&store, "t-unknown").expect_err("missing");
    assert_eq!(
        error
            .downcast_ref::<crate::execution::Refusal>()
            .expect("refusal")
            .code,
        "not_found"
    );
}

#[test]
fn caller_owned_receipt_records_lost_worker_before_reading_snapshot() {
    let (_home, _guard, store) = setup();
    let mut task = crate::cost::test_support::task(AgentKind::Codex, 0, None);
    task.id = TaskId("t-lost-receipt".into());
    store.insert_task(&task).expect("task");
    let mut spec: background::BackgroundRunSpec =
        serde_json::from_str(include_str!("../../testdata/legacy-background-spec.json")).expect("spec");
    spec.task_id = task.id.0.clone();
    spec.worker_pid = Some(u32::MAX);
    spec.agent_pid = None;
    background::save_spec(&spec).expect("spec");
    let observed = receipt(&store, task.id.as_str()).expect("receipt");
    assert_eq!(observed["settled"], true);
    assert_eq!(observed["worker"]["outcome"], "lost");
    assert!(observed["revision"].as_i64().expect("revision") > 1);
    assert_eq!(
        store
            .execution_snapshot(task.id.as_str())
            .expect("snapshot")
            .expect("row")["revision"],
        observed["revision"]
    );
}
