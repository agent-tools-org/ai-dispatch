// Settlement persistence failures must not bypass existing worker and reaper cleanup.
// SQLite abort triggers isolate the new settlement write from ordinary lifecycle mutations.
use super::*;
use crate::support::test_env::TestHome;
use crate::types::{Task, TaskStatus};

fn reject_settlement(store: &Store) {
    store
        .db()
        .execute_batch(
            "CREATE TRIGGER reject_settlement BEFORE UPDATE OF worker_settlement ON tasks
         BEGIN SELECT RAISE(ABORT, 'settlement unavailable'); END;",
        )
        .expect("inject settlement failure");
}

#[tokio::test]
async fn caller_owned_worker_settlement_failure_still_clears_spec_and_input_files() {
    let (_home, store) = TestHome::with_dirs().shared_store();
    let task = Task::test_fixture(TaskId("t-cleanup-settlement".into()), TaskStatus::Running);
    store.insert_task(&task).expect("task");
    save_spec(&BackgroundRunSpec {
        task_id: task.id.to_string(),
        ..Default::default()
    })
    .expect("spec");
    crate::runtime::input_signal::write_response(task.id.as_str(), "response").expect("response");
    crate::runtime::input_signal::write_steer(task.id.as_str(), "steer").expect("steer");
    reject_settlement(&store);
    let error = run_task(store.clone(), task.id.as_str())
        .await
        .expect_err("missing dispatch args");
    assert!(error.to_string().contains("no saved dispatch args"), "{error:#}");
    assert!(load_spec_if_exists(task.id.as_str()).expect("spec").is_none());
    assert!(
        crate::runtime::input_signal::take_response(task.id.as_str())
            .expect("response")
            .is_none()
    );
    assert!(
        crate::runtime::input_signal::take_steer(task.id.as_str())
            .expect("steer")
            .is_none()
    );
    assert_eq!(
        store
            .get_task(task.id.as_str())
            .expect("task")
            .expect("row")
            .status,
        TaskStatus::Failed
    );
}

#[test]
fn caller_owned_reaper_settlement_failure_still_kills_agent_and_fails_row() {
    let (_home, store) = TestHome::with_dirs().shared_store();
    let task = Task::test_fixture(TaskId("t-reap-settlement".into()), TaskStatus::Running);
    store.insert_task(&task).expect("task");
    let spec = BackgroundRunSpec {
        task_id: task.id.to_string(),
        worker_pid: Some(202),
        agent_pid: Some(303),
        ..Default::default()
    };
    save_spec(&spec).expect("spec");
    reject_settlement(&store);
    kill::RECORDED_KILLS.with(|kills| kills.borrow_mut().clear());
    let mut cleaned = Vec::new();
    reaper::cleanup_pid_task(&store, &task, &spec, 202, &mut cleaned, &|_| false, 60).expect("cleanup");
    assert_eq!(cleaned, vec![task.id.to_string()]);
    assert_eq!(
        kill::RECORDED_KILLS.with(|kills| kills.borrow().clone()),
        vec![vec![202, 303]]
    );
    assert_eq!(
        store
            .get_task(task.id.as_str())
            .expect("task")
            .expect("row")
            .status,
        TaskStatus::Failed
    );
    assert!(
        store
            .latest_error(task.id.as_str())
            .expect("error")
            .contains("worker disappeared")
    );
}
