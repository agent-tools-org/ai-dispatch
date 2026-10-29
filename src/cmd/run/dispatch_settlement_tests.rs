// A failed launch settlement write must not suppress the existing preparation failure path.
// The task status and original error event remain authoritative even when settlement storage fails.
use super::*;

#[test]
fn caller_owned_preparation_settlement_failure_still_fails_pending_row() {
    let (_home, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    let task = Task::test_fixture(TaskId("t-preparation-settlement".into()), TaskStatus::Pending);
    store.insert_task(&task).expect("task");
    store
        .db()
        .execute_batch(
            "CREATE TRIGGER reject_settlement BEFORE UPDATE OF worker_settlement ON tasks
         BEGIN SELECT RAISE(ABORT, 'settlement unavailable'); END;",
        )
        .expect("inject settlement failure");
    record_error(&store, &task.id, &anyhow::anyhow!("original preparation failure"))
        .expect("existing status and error cleanup still run");
    let row = store.get_task(task.id.as_str()).expect("task").expect("row");
    assert_eq!(row.status, TaskStatus::Failed);
    assert_eq!(row.pending_reason.as_deref(), Some("preparation"));
    assert_eq!(
        store.latest_error(task.id.as_str()).as_deref(),
        Some("original preparation failure")
    );
    assert_ne!(
        store
            .execution_facts(task.id.as_str())
            .expect("facts")
            .expect("row")
            .settlement
            .expect("worker state")
            .state,
        "settled"
    );
}
