// Manual retry timeout overrides through actual dispatch and worker completion.
// Shares retry fixtures; fake custom agent avoids external provider calls.
use super::*;
use crate::types::DispatchOrigin;

#[test]
fn explicit_retry_launch_claim_uses_persisted_origin_after_parent_stop() {
    for origin in [DispatchOrigin::Automatic, DispatchOrigin::ExplicitRetry] {
        let store = Store::open_memory().unwrap();
        let mut parent = failed_task("t-origin-parent");
        parent.status = TaskStatus::Running;
        store.insert_task(&parent).unwrap();
        let mut child = failed_task("t-origin-child");
        child.status = TaskStatus::Running;
        child.parent_task_id = Some(parent.id.to_string());
        store.insert_task_with_origin(&child, origin).unwrap();
        assert!(crate::task_lifecycle::mark_stopped(&store, parent.id.as_str()).unwrap());
        let mut launched = false;

        let result = store.with_worker_launch(child.id.as_str(), || {
            launched = true;
            Ok(())
        });

        let explicit = origin == DispatchOrigin::ExplicitRetry;
        assert_eq!(result.is_ok(), explicit);
        assert_eq!(launched, explicit);
        let expected = if explicit {
            TaskStatus::Running
        } else {
            TaskStatus::Stopped
        };
        assert_eq!(
            store.get_task(child.id.as_str()).unwrap().unwrap().status,
            expected
        );
    }
}

#[test]
fn explicit_retry_waiting_replacement_of_stopped_parent_launches() {
    let store = Store::open_memory().unwrap();
    let mut parent = failed_task("t-waiting-parent");
    parent.status = TaskStatus::Stopped;
    store.insert_task(&parent).unwrap();
    let mut child = failed_task("t-waiting-child");
    child.status = TaskStatus::Waiting;
    store.insert_task(&child).unwrap();
    child.parent_task_id = Some(parent.id.to_string());
    child.status = TaskStatus::Pending;

    store
        .replace_waiting_task_with_origin(&child, DispatchOrigin::ExplicitRetry)
        .unwrap();
    assert!(crate::task_lifecycle::mark_running(&store, &child.id).unwrap());
    let mut launched = false;
    store
        .with_worker_launch(child.id.as_str(), || {
            launched = true;
            Ok(())
        })
        .unwrap();

    assert!(launched);
    assert_eq!(
        store.get_task(child.id.as_str()).unwrap().unwrap().status,
        TaskStatus::Running
    );
}

#[test]
fn explicit_retry_origin_does_not_bypass_child_own_stop() {
    let store = Store::open_memory().unwrap();
    let mut parent = failed_task("t-own-stop-parent");
    parent.status = TaskStatus::Stopped;
    store.insert_task(&parent).unwrap();
    let mut child = failed_task("t-own-stop-child");
    child.status = TaskStatus::Running;
    child.parent_task_id = Some(parent.id.to_string());
    store
        .insert_task_with_origin(&child, DispatchOrigin::ExplicitRetry)
        .unwrap();
    assert!(crate::task_lifecycle::mark_stopped(&store, child.id.as_str()).unwrap());
    let mut launched = false;

    assert!(
        store
            .with_worker_launch(child.id.as_str(), || {
                launched = true;
                Ok(())
            })
            .is_err()
    );

    assert!(!launched);
    assert_eq!(
        store.get_task(child.id.as_str()).unwrap().unwrap().status,
        TaskStatus::Stopped
    );
}

fn explicit_retry_setup(
    status: TaskStatus,
) -> (
    tempfile::TempDir,
    crate::support::paths::AidHomeGuard,
    std::sync::Arc<Store>,
    Task,
) {
    let home = tempfile::tempdir().unwrap();
    let guard = crate::support::paths::AidHomeGuard::set(home.path());
    crate::support::paths::ensure_dirs().unwrap();
    std::fs::create_dir_all(crate::support::paths::aid_dir().join("agents")).unwrap();
    std::fs::write(
        crate::support::paths::aid_dir().join("agents/explicit-retry.toml"),
        concat!(
            "[agent]\nid = \"explicit-retry\"\ndisplay_name = \"Explicit Retry\"\n",
            "command = \"/bin/sh\"\nprompt_mode = \"arg\"\n",
            "fixed_args = [\"-c\", \"echo retried-with-feedback\"]\ninteractive_input = false\n"
        ),
    )
    .unwrap();
    let store = std::sync::Arc::new(Store::open_memory().unwrap());
    let mut task = failed_task("t-explicit-parent");
    task.status = status;
    task.agent = AgentKind::Custom;
    task.custom_agent_name = Some("explicit-retry".into());
    task.requested_model = None;
    task.effective_dir = Some(home.path().display().to_string());
    (home, guard, store, task)
}

async fn assert_explicit_retry_launched(store: std::sync::Arc<Store>, task: &Task) {
    let saved = RunArgs {
        agent_name: "explicit-retry".into(),
        prompt: task.prompt.clone(),
        read_only: true,
        dir: task.effective_dir.clone(),
        ..Default::default()
    };
    insert_with_saved(&store, task, &saved);
    let retry_id = crate::cmd::retry::retry_task(store.clone(), base_retry(task.id.as_str()), false)
        .await
        .expect("explicit retry with feedback must launch");
    let child = store.get_task(retry_id.as_str()).unwrap().unwrap();
    assert_eq!(child.status, TaskStatus::Done);
    assert_eq!(child.parent_task_id.as_deref(), Some(task.id.as_str()));
    assert!(child.prompt.contains("fix it"));
    assert_eq!(
        RunArgs::saved_for_task(&store, retry_id.as_str()).unwrap().unwrap().origin,
        DispatchOrigin::ExplicitRetry
    );
    assert_eq!(
        store.get_task(task.id.as_str()).unwrap().unwrap().status,
        TaskStatus::Stopped
    );
    let log = std::fs::read_to_string(crate::support::paths::log_path(retry_id.as_str())).unwrap();
    assert!(
        log.contains("retried-with-feedback"),
        "worker must run the fake agent: {log}"
    );
}

#[tokio::test]
async fn explicit_retry_stopped_parent_with_feedback_launches() {
    let (_home, _guard, store, task) = explicit_retry_setup(TaskStatus::Stopped);
    assert_explicit_retry_launched(store, &task).await;
}

#[tokio::test]
async fn explicit_retry_supersedes_running_parent_and_launches() {
    let (home, _guard, store, mut task) = explicit_retry_setup(TaskStatus::Running);
    task.worktree_path = Some(home.path().display().to_string());
    std::fs::write(
        home.path().join(".aid-lock"),
        format!(
            "version=1\ntask_id={}\nowner_pid={}\nworker_pid=\n",
            task.id,
            std::process::id()
        ),
    )
    .unwrap();
    let mut retry = base_retry(task.id.as_str());
    retry.dir = task.effective_dir.clone();
    // The directory override avoids Git setup; supersede still checks the recorded lease.
    let saved = RunArgs {
        agent_name: "explicit-retry".into(),
        prompt: task.prompt.clone(),
        read_only: true,
        dir: task.effective_dir.clone(),
        ..Default::default()
    };
    insert_with_saved(&store, &task, &saved);
    let child_id = crate::cmd::retry::retry_task(store.clone(), retry, false)
        .await
        .expect("retry must launch after stopping its own running parent");
    assert_eq!(
        store.get_task(task.id.as_str()).unwrap().unwrap().status,
        TaskStatus::Stopped
    );
    assert!(!home.path().join(".aid-lock").exists());
    assert_eq!(
        store.get_task(child_id.as_str()).unwrap().unwrap().status,
        TaskStatus::Done
    );
    let log = std::fs::read_to_string(crate::support::paths::log_path(child_id.as_str())).unwrap();
    assert!(log.contains("retried-with-feedback"));
}

#[tokio::test]
async fn idle_override_recomputes_first_token_and_preserves_exact_hard_timeout() {
    let home = tempfile::tempdir().unwrap();
    let _guard = crate::support::paths::AidHomeGuard::set(home.path());
    crate::support::paths::ensure_dirs().unwrap();
    std::fs::create_dir_all(crate::support::paths::aid_dir().join("agents")).unwrap();
    std::fs::write(crate::support::paths::aid_dir().join("agents/retry-policy.toml"),
        "[agent]\nid = \"retry-policy\"\ndisplay_name = \"Retry Policy\"\ncommand = \"/bin/sh\"\nprompt_mode = \"arg\"\nfixed_args = [\"-c\", \"echo completed\"]\ninteractive_input = false\n").unwrap();
    let store = std::sync::Arc::new(Store::open_memory().unwrap());
    let mut task = failed_task("t-idle-dispatch");
    task.agent = AgentKind::Custom;
    task.custom_agent_name = Some("retry-policy".into());
    task.requested_model = None;
    let mut saved = RunArgs {
        agent_name: "retry-policy".into(), prompt: task.prompt.clone(),
        idle_timeout_secs: Some(300), timeout: Some(71), read_only: true,
        env: Some(std::collections::HashMap::from([("TOKEN".into(), "do-not-replay".into())])),
        ..Default::default()
    };
    saved.timeout_policy.idle = std::time::Duration::from_secs(300);
    saved.timeout_policy.first_token = std::time::Duration::from_secs(300);
    saved.timeout_policy.max_duration = std::time::Duration::from_secs(71);
    insert_with_saved(&store, &task, &saved);
    let mut retry = base_retry(task.id.as_str());
    retry.idle_timeout_secs = Some(900);
    let retry_id = crate::cmd::retry::retry_task(store.clone(), retry, false).await.unwrap();
    let actual = RunArgs::saved_for_task(&store, retry_id.as_str()).unwrap().unwrap();
    assert_eq!(actual.timeout_policy.idle.as_secs(), 900);
    assert_eq!(actual.timeout_policy.first_token.as_secs(), 900);
    assert_eq!(actual.timeout_policy.max_duration.as_secs(), 71);
    assert!(actual.env.is_none());
    assert_eq!(store.get_task(retry_id.as_str()).unwrap().unwrap().status, TaskStatus::Done);
}
