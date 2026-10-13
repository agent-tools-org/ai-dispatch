// Pending capacity continuation cancellation through direct and retry-tree stop.
// Exercises persisted cancellation and the worker's final launch check.

use super::*;

#[test]
fn provider_capacity_regression_stop_cancels_backoff_with_existing_child() {
    let home = tempfile::tempdir().expect("home");
    let _home = paths::AidHomeGuard::set(home.path());
    let store = Arc::new(Store::open_memory().expect("store"));
    let row = task("t-capacity-parent", home.path());
    let mut child = task("t-capacity-existing-child", home.path());
    child.parent_task_id = Some(row.id.to_string());
    child.status = TaskStatus::Done;
    store.insert_task(&row).expect("parent");
    store.insert_task(&child).expect("child");
    let mut waiting = task("t-capacity-waiting-child", home.path());
    waiting.parent_task_id = Some(row.id.to_string());
    waiting.status = TaskStatus::Waiting;
    store.insert_task(&waiting).expect("waiting child");
    crate::cmd::stop::stop_retry_tree(&store, row.id.as_str(), false).expect("cancel waiting child");
    assert_eq!(
        store
            .get_task(waiting.id.as_str())
            .expect("task")
            .expect("row")
            .status,
        TaskStatus::Stopped
    );
    assert_eq!(
        store
            .get_task(child.id.as_str())
            .expect("task")
            .expect("row")
            .status,
        TaskStatus::Done
    );
}

#[test]
fn provider_capacity_stop_rejects_backoff_that_already_ended() {
    let home = tempfile::tempdir().expect("home");
    let _home = paths::AidHomeGuard::set(home.path());
    let store = Arc::new(Store::open_memory().expect("store"));
    let row = task("t-capacity-ended", home.path());
    store.insert_task(&row).expect("task");
    record(&store, &row.id, "capacity backoff".into(), true).expect("pending");
    record(&store, &row.id, "backoff ended".into(), false).expect("ended");
    assert!(crate::cmd::stop::stop(&store, row.id.as_str()).is_err());
    assert_eq!(
        store
            .get_task(row.id.as_str())
            .expect("task")
            .expect("row")
            .status,
        TaskStatus::Failed
    );
}

async fn cancel_waiting(mode: u8) {
    let home = tempfile::tempdir().expect("home");
    let _home = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("dirs");
    install_agent(home.path(), "capacity-cancel", "echo should-not-launch");
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut row = task("t-capacity-cancel", home.path());
    row.agent = AgentKind::Custom;
    row.custom_agent_name = Some("capacity-cancel".into());
    store.insert_task(&row).expect("task");
    let refusal = serde_json::json!({"type": "error", "message": CAPACITY});
    seed_stdout(&store, &row.id, &refusal.to_string());
    let args = RunArgs {
        agent_name: "capacity-cancel".into(),
        dir: row.effective_dir.clone(),
        no_audit: true,
        skills: vec![crate::cmd::run::NO_SKILL_SENTINEL.into()],
        ..Default::default()
    };
    let worker = continue_capacity(&store, &args, &row.id);
    let stopper = stop_after_marker(&store, row.id.as_str(), mode);
    let (outcome, ()) = tokio::join!(worker, stopper);
    let child_id = outcome.expect("worker").expect("waiting retry");
    assert_eq!(
        store
            .get_task(child_id.as_str())
            .expect("task")
            .expect("row")
            .status,
        TaskStatus::Stopped
    );
    assert_eq!(
        store.get_retry_tree(row.id.as_str()).expect("tree").len(),
        2
    );
}

async fn stop_after_marker(store: &Arc<Store>, task_id: &str, mode: u8) {
    let child = loop {
        if let Some(child) = store.get_retry_tree(task_id).expect("tree").into_iter()
            .find(|task| task.parent_task_id.as_deref() == Some(task_id))
        { break child; }
        tokio::task::yield_now().await;
    };
    if mode == 2 {
        store
            .db()
            .execute(
                "UPDATE tasks SET status = 'stopped' WHERE id = ?1",
                [child.id.as_str()],
            )
            .expect("persist cancellation");
    } else if mode == 1 {
        crate::cmd::stop::stop_retry_tree(store, task_id, false).expect("tree stop");
    } else {
        crate::cmd::stop::stop(store, child.id.as_str()).expect("direct stop");
    }
}

#[tokio::test]
async fn provider_capacity_regression_direct_stop_cancels_backoff() {
    tokio::time::timeout(std::time::Duration::from_secs(15), cancel_waiting(0))
        .await
        .expect("bounded cancellation");
}

#[tokio::test]
async fn provider_capacity_regression_retry_tree_stop_cancels_backoff() {
    tokio::time::timeout(std::time::Duration::from_secs(15), cancel_waiting(1))
        .await
        .expect("bounded cancellation");
}

#[tokio::test]
async fn provider_capacity_regression_worker_rechecks_cancellation_before_launch() {
    tokio::time::timeout(std::time::Duration::from_secs(15), cancel_waiting(2))
        .await
        .expect("bounded cancellation");
}
