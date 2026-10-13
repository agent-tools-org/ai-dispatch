// Explicit batch retry of stopped parents through worker completion.
// Deps: shared retry fixtures, isolated Git directory and fake custom agent.
use super::*;

#[tokio::test]
async fn explicit_retry_batch_stopped_parent_launches() {
    let home = TempDir::new().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().unwrap();
    let repo = TempDir::new().unwrap();
    install_explicit_retry_agent(repo.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = make_task("t-batch-stopped", AgentKind::Custom);
    task.custom_agent_name = Some("batch-explicit".into());
    task.status = TaskStatus::Stopped;
    task.workgroup_id = Some("wg-explicit".into());
    task.read_only = true;
    set_repo(&mut task, &repo);
    store.insert_task(&task).unwrap();

    retry_failed(store.clone(), "wg-explicit", None, false)
        .await
        .unwrap();

    let children = store.list_tasks(crate::types::TaskFilter::All).unwrap();
    let child = children
        .iter()
        .find(|row| row.parent_task_id.as_deref() == Some(task.id.as_str()))
        .expect("explicit batch retry must create a child of the stopped task");
    let outcome = wait_for_task_ids(
        &store,
        &[child.id.to_string()],
        None,
        false,
        Some(Duration::from_secs(10)),
    )
    .await
    .unwrap();
    assert!(matches!(outcome, WaitOutcome::Completed));
    assert_eq!(
        store.get_task(child.id.as_str()).unwrap().unwrap().status,
        TaskStatus::Done
    );
    assert_eq!(
        store.get_task(task.id.as_str()).unwrap().unwrap().status,
        TaskStatus::Stopped
    );
    let log = std::fs::read_to_string(paths::log_path(child.id.as_str())).unwrap();
    assert!(log.contains("batch-retried"));
}

fn install_explicit_retry_agent(repo: &std::path::Path) {
    let _permit = crate::test_subprocess::acquire();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(repo)
            .output()
            .unwrap()
            .status
            .success()
    );
    std::fs::create_dir_all(paths::aid_dir().join("agents")).unwrap();
    std::fs::write(
        paths::aid_dir().join("agents/batch-explicit.toml"),
        concat!(
            "[agent]\nid = \"batch-explicit\"\ndisplay_name = \"Batch Explicit\"\n",
            "command = \"/bin/sh\"\nprompt_mode = \"arg\"\n",
            "fixed_args = [\"-c\", \"echo batch-retried\"]\ninteractive_input = false\n"
        ),
    )
    .unwrap();
}
