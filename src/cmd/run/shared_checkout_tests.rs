// Integration of shared-checkout preservation with task settlement.
// Uses the lifecycle fixture helpers; never launches an agent.
use super::*;

fn output(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

#[tokio::test]
async fn shared_settlement_keeps_dirty_work_and_records_recovery_for_done_and_failed_tasks() {
    let _permit = test_subprocess::acquire();
    for status in [TaskStatus::Done, TaskStatus::Failed] {
        let dir = init_repo();
        git(dir.path(), &["config", "user.name", "Test"]);
        git(dir.path(), &["config", "user.email", "test@example.invalid"]);
        write_path(dir.path(), "base.txt", "base");
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "base"]);
        let head = output(dir.path(), &["rev-parse", "HEAD"]);
        write_path(dir.path(), "result.txt", "task result");
        let store = Arc::new(Store::open_memory().unwrap());
        let row = task("t-shared-settlement", status);
        store.insert_task(&row).unwrap();
        let action = post_agent_dirty_worktree_cleanup(
            &store, &row.id, &RunArgs::default(), dir.path().to_str().unwrap(), None,
        ).await.unwrap();
        assert_eq!(action, DirtyWorktreeAction::Continue);
        assert_eq!(store.get_task(row.id.as_str()).unwrap().unwrap().status, status);
        assert_eq!(output(dir.path(), &["rev-parse", "HEAD"]), head);
        assert_eq!(output(dir.path(), &["status", "--porcelain"]), "?? result.txt");
        let events = store.get_events(row.id.as_str()).unwrap();
        let metadata = events.iter().find_map(|event| event.metadata.as_ref()
            .filter(|data| data.get("recovery_ref").is_some())).unwrap();
        let reference = metadata["recovery_ref"].as_str().unwrap();
        assert_eq!(output(dir.path(), &["show", &format!("{reference}:result.txt")]), "task result");
    }
}

#[tokio::test]
async fn shared_checkpoint_failure_marks_task_failed_without_commit_or_retry() {
    let _permit = test_subprocess::acquire();
    let dir = init_repo();
    git(dir.path(), &["config", "user.name", "Test"]);
    git(dir.path(), &["config", "user.email", "test@example.invalid"]);
    write_path(dir.path(), "base.txt", "base");
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "base"]);
    let head = output(dir.path(), &["rev-parse", "HEAD"]);
    git(dir.path(), &["update-ref", "refs/aid/recovery/t-shared-fail", &head]);
    write_path(dir.path(), "result.txt", "keep me");
    let store = Arc::new(Store::open_memory().unwrap());
    let row = task("t-shared-fail", TaskStatus::Done);
    store.insert_task(&row).unwrap();
    let action = post_agent_dirty_worktree_cleanup(
        &store, &row.id, &RunArgs::default(), dir.path().to_str().unwrap(), None,
    ).await.unwrap();
    assert_eq!(action, DirtyWorktreeAction::Failed);
    assert_eq!(store.get_task(row.id.as_str()).unwrap().unwrap().status, TaskStatus::Failed);
    assert_eq!(output(dir.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(std::fs::read_to_string(dir.path().join("result.txt")).unwrap(), "keep me");
}

#[tokio::test]
async fn read_only_shared_checkout_never_creates_a_recovery_ref() {
    let _permit = test_subprocess::acquire();
    let dir = init_repo();
    write_path(dir.path(), "result.txt", "read only");
    let store = Arc::new(Store::open_memory().unwrap());
    let row = task("t-shared-readonly", TaskStatus::Done);
    store.insert_task(&row).unwrap();
    let args = RunArgs { read_only: true, ..Default::default() };
    let action = post_agent_dirty_worktree_cleanup(
        &store, &row.id, &args, dir.path().to_str().unwrap(), None,
    ).await.unwrap();
    assert_eq!(action, DirtyWorktreeAction::Continue);
    assert!(store.get_events(row.id.as_str()).unwrap().is_empty());
    assert_eq!(output(dir.path(), &["for-each-ref", "--format=%(refname)", "refs/aid/recovery/"]), "");
}

#[tokio::test]
async fn shared_checkpoint_does_not_skip_failed_verification() {
    let _permit = test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let _home_guard = crate::paths::AidHomeGuard::set(home.path());
    let dir = init_repo();
    write_path(dir.path(), "result.txt", "task output");
    let store = Arc::new(Store::open_memory().unwrap());
    let mut row = task("t-shared-verify", TaskStatus::Done);
    row.verify = Some("false".to_string());
    row.repo_path = Some(dir.path().display().to_string());
    store.insert_task(&row).unwrap();
    let args = RunArgs {
        dir: row.repo_path.clone(), repo: row.repo_path.clone(),
        verify: row.verify.clone(), ..Default::default()
    };
    post_run_lifecycle(
        LifecycleMode::Background, &store, &row.id, &args, AgentKind::Codex,
        "codex", args.dir.as_ref(), args.repo.as_ref(), None, None, &[],
        &prompt_bundle(), TaskStatus::Done, None,
    ).await.unwrap();
    let settled = store.get_task(row.id.as_str()).unwrap().unwrap();
    assert_eq!(settled.verify_status, VerifyStatus::Failed);
    assert_eq!(settled.status, TaskStatus::Failed);
    assert_eq!(std::fs::read_to_string(dir.path().join("result.txt")).unwrap(), "task output");
    assert!(!dir.path().join(".git/index").exists());
    assert!(store.get_events(row.id.as_str()).unwrap().iter().any(|event|
        event.metadata.as_ref().is_some_and(|data| data.get("recovery_ref").is_some())));
}

#[tokio::test]
async fn mismatched_task_worktree_is_rejected_before_rescue() {
    let _permit = test_subprocess::acquire();
    let owned = init_repo();
    let shared = init_repo();
    write_path(shared.path(), "result.txt", "do not commit");
    let store = Arc::new(Store::open_memory().unwrap());
    let mut row = task("t-wrong-checkout", TaskStatus::Done);
    row.worktree_path = Some(owned.path().display().to_string());
    store.insert_task(&row).unwrap();
    let error = post_agent_dirty_worktree_cleanup(
        &store, &row.id, &RunArgs::default(), shared.path().to_str().unwrap(), None,
    ).await.unwrap_err();
    assert!(error.to_string().contains("does not match settlement directory"));
    assert!(!shared.path().join(".git/index").exists());
    assert!(shared.path().join("result.txt").exists());
}
