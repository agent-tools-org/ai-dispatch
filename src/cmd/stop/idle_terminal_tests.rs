// Terminal rows without a live task process retain artifacts, locks and task metadata.
// Text mode preserves the terminal error; JSON independently acknowledges worker settlement.
use super::*;
use crate::support::test_env::{TestHome, git_stdout};

fn assert_idle_terminal(status: TaskStatus, outcome: Option<&str>) {
    let (_home, store) = TestHome::with_dirs().shared_store();
    let repo = crate::support::test_repo::repo_with_file("base.txt", "base\n", "base");
    let mut task = Task::test_fixture(TaskId("t-idle-terminal".into()), status);
    task.completed_at = Some(chrono::Local::now());
    task.worktree_path = Some(repo.path().display().to_string());
    task.repo_path = task.worktree_path.clone();
    store.insert_task(&task).expect("task");
    store
        .insert_event(&TaskEvent::new(
            task.id.clone(),
            EventKind::Error,
            "previous failure".to_string(),
        ))
        .expect("event");
    if let Some(outcome) = outcome {
        store
            .settle_worker(task.id.as_str(), outcome, None)
            .expect("settle");
    }
    super::settle_first_tests::save_worker(task.id.as_str(), u32::MAX);
    crate::workspace::worktree::try_acquire_worktree_lock(repo.path(), task.id.as_str()).expect("lock");
    std::fs::write(repo.path().join("base.txt"), "uncommitted\n").expect("dirty file");
    let head = git_stdout(repo.path(), &["rev-parse", "HEAD"]);
    let dirty = git_stdout(repo.path(), &["status", "--porcelain"]);
    let lock = std::fs::read(repo.path().join(".aid-lock")).expect("lock bytes");
    let error = stop(&store, task.id.as_str()).expect_err("text terminal refusal");
    assert_eq!(
        error.to_string(),
        format!("task already {}: previous failure", status.as_str())
    );
    store.test_caller_owned(task.id.as_str());
    assert_eq!(stop_json(&store, task.id.as_str(), true).expect("JSON stop"), 0);
    assert_eq!(
        git_stdout(repo.path(), &["rev-parse", "HEAD"]),
        head,
        "no auto-commit"
    );
    assert_eq!(git_stdout(repo.path(), &["status", "--porcelain"]), dirty);
    assert_eq!(
        std::fs::read(repo.path().join(".aid-lock")).expect("lock kept"),
        lock
    );
    assert_idle_metadata(&store, &task, outcome);
}

fn assert_idle_metadata(store: &Store, task: &Task, outcome: Option<&str>) {
    let after = store.get_task(task.id.as_str()).expect("task").expect("row");
    assert_eq!(after.status, task.status);
    assert_eq!(after.completed_at, task.completed_at);
    assert!(after.final_head_sha.is_none(), "no final-state capture");
    assert!(after.final_branch.is_none());
    assert!(after.duration_ms.is_none());
    assert_eq!(store.get_events(task.id.as_str()).expect("events").len(), 1);
    assert!(
        background::load_spec_if_exists(task.id.as_str())
            .expect("spec")
            .is_some() == outcome.is_some()
    );
    let facts = store
        .execution_facts(task.id.as_str())
        .expect("facts")
        .expect("row");
    assert_eq!(
        facts.settlement.expect("settlement").outcome.as_deref(),
        Some(outcome.unwrap_or("stopped"))
    );
}

#[test]
fn terminal_without_live_process_preserves_artifacts_locks_and_existing_settlement() {
    let _permit = crate::support::test_subprocess::acquire();
    for status in [
        TaskStatus::Done,
        TaskStatus::Failed,
        TaskStatus::Stopped,
        TaskStatus::Merged,
        TaskStatus::Skipped,
    ] {
        for outcome in [None, Some("completed"), Some("lost")] {
            assert_idle_terminal(status, outcome);
        }
    }
}

#[cfg(unix)]
#[test]
fn terminal_reused_pid_is_not_signalled_and_does_not_block_json_settlement() {
    let _permit = crate::support::test_subprocess::acquire();
    let (_home, store) = TestHome::with_dirs().shared_store();
    let mut task = Task::test_fixture(TaskId("t-terminal-reused".into()), TaskStatus::Failed);
    task.completed_at = Some(chrono::Local::now());
    store.insert_task(&task).expect("task");
    let mut command = std::process::Command::new("sleep");
    command.arg("60");
    let mut child = crate::runtime::process::guard::ProcessGuard::spawn(&mut command).expect("foreign child");
    super::settle_first_tests::save_worker(task.id.as_str(), child.id());
    store.test_caller_owned(task.id.as_str());
    assert_eq!(stop_json(&store, task.id.as_str(), true).expect("JSON stop"), 0);
    store.db().execute("UPDATE tasks SET execution_policy=NULL WHERE id=?1", [task.id.as_str()])
        .expect("default text parity");
    let error = stop(&store, task.id.as_str()).expect_err("idle terminal text refusal");
    assert!(error.to_string().starts_with("task already failed:"));
    assert!(
        child.child_mut().try_wait().expect("owner wait").is_none(),
        "foreign process survived"
    );
    let facts = store
        .execution_facts(task.id.as_str())
        .expect("facts")
        .expect("row");
    assert_eq!(
        facts.settlement.expect("settled").outcome.as_deref(),
        Some("stopped")
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
        background::load_spec_if_exists(task.id.as_str())
            .expect("spec")
            .is_none()
    );
}
