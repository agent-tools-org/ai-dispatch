// Shared waiting-task cancellation through direct and retry-tree stop.
// Explicit waiting fixtures cover stopped-row replacement and suffixed IDs.

use super::*;

#[test]
fn provider_capacity_explicit_waiting_stop_preserves_completed_child() {
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
fn provider_capacity_stop_rejects_terminal_parent_without_recovery() {
    let home = tempfile::tempdir().expect("home");
    let _home = paths::AidHomeGuard::set(home.path());
    let store = Arc::new(Store::open_memory().expect("store"));
    let row = task("t-capacity-ended", home.path());
    store.insert_task(&row).expect("task");
    seed_stdout(&store, &row.id,
        &serde_json::json!({"type": "error", "message": CAPACITY}).to_string());
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
    let mut waiting = row.clone();
    waiting.id = TaskId::generate();
    waiting.parent_task_id = Some(row.id.to_string());
    waiting.status = TaskStatus::Waiting;
    store.insert_task(&waiting).expect("ordinary waiting fixture");
    stop_after_marker(&store, row.id.as_str(), mode).await;
    let mut child = args;
    child.parent_task_id = Some(row.id.to_string());
    child.existing_task_id = Some(waiting.id.clone());
    assert!(crate::cmd::run::run(store.clone(), child).await.is_err());
    let child_id = waiting.id;
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
async fn provider_capacity_explicit_waiting_direct_stop_prevents_dispatch() {
    tokio::time::timeout(std::time::Duration::from_secs(15), cancel_waiting(0))
        .await
        .expect("bounded cancellation");
}

#[tokio::test]
async fn provider_capacity_explicit_waiting_tree_stop_prevents_dispatch() {
    tokio::time::timeout(std::time::Duration::from_secs(15), cancel_waiting(1))
        .await
        .expect("bounded cancellation");
}

#[tokio::test]
async fn provider_capacity_explicit_waiting_persisted_cancellation_prevents_dispatch() {
    tokio::time::timeout(std::time::Duration::from_secs(15), cancel_waiting(2))
        .await
        .expect("bounded cancellation");
}
