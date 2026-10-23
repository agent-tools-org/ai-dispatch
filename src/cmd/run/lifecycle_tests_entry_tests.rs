// Production entry point regression tests split to keep the parent within its size limit.
// Deps: parent test fixtures and production APIs.

use super::*;

#[tokio::test]
async fn background_lifecycle_runs_on_fail_hook() {
    let _permit = test_subprocess::acquire();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-bg-lifecycle-fail".to_string());
    store
        .insert_task(&task(task_id.as_str(), TaskStatus::Failed))
        .unwrap();
    let hook_path = dir.path().join("hook.txt");
    let hook = Hook::new_trusted(
        "on_fail".to_string(),
        format!("printf failed > '{}'", hook_path.display()),
        None,
    );

    post_run_lifecycle(
        LifecycleMode::Background,
        &store,
        &task_id,
        &RunArgs::default(),
        AgentKind::Codex,
        "codex",
        None,
        None,
        None,
        None,
        &[hook],
        &prompt_bundle(),
        TaskStatus::Failed,
        None,
    )
    .await
    .unwrap();

    assert_eq!(std::fs::read_to_string(hook_path).unwrap(), "failed");
}
