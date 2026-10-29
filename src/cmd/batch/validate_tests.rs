// Tests for batch validation helpers and dependency readiness.
// Exports: none.
// Deps: super::batch_validate, crate::batch, crate::routing::rate_limit, crate::store
use super::*;
use crate::support::paths::AidHomeGuard;
use crate::routing::rate_limit;
use crate::store::Store;
use crate::types::TaskOutcome;
use std::sync::Arc;
use tempfile::TempDir;

fn stub_task(name: &str, depends_on: Option<Vec<&str>>) -> batch::BatchTask {
    batch::BatchTask {
        name: Some(name.to_string()),
        agent: "codex".to_string(),
        prompt: "test".to_string(),
        depends_on: depends_on.map(|values| values.into_iter().map(String::from).collect()),
        ..Default::default()
    }
}

#[test]
fn parallel_same_dir_without_worktree_errors() {
    let mut first = stub_task("first", None);
    first.dir = Some("src".to_string());
    let mut second = stub_task("second", None);
    second.dir = Some("src".to_string());

    let err = validate_batch_config(&[first, second], true, false)
        .unwrap_err()
        .to_string();

    assert_eq!(
        err,
        "Error: 2 tasks target 'src' without worktree isolation. This causes git index.lock contention. Add `worktree = \"branch-name\"` to each task, or use `--force` to override."
    );
}

#[test]
fn parallel_same_dir_with_force_warns() {
    let _temp = crate::support::test_env::TestHome::new();
    let mut first = stub_task("first", None);
    first.dir = Some("src".to_string());
    let mut second = stub_task("second", None);
    second.dir = Some("src".to_string());

    assert!(validate_batch_config(&[first, second], true, true).is_ok());

    let mut warning_tasks = [stub_task("first", None), stub_task("second", None)];
    warning_tasks[0].dir = Some("src".to_string());
    warning_tasks[1].dir = Some("src".to_string());
    let conflicts = shared_dir_conflicts(&warning_tasks);
    assert_eq!(conflicts.len(), 1);
    let warning = parallel_dir_conflict_warning(&conflicts[0]);
    assert_eq!(
        warning,
        "[aid] Warning: 2 tasks target 'src' without worktree isolation. This causes git index.lock contention. Proceeding because --force is set."
    );
}

#[test]
fn parallel_same_dir_readonly_excluded() {
    let _temp = crate::support::test_env::TestHome::new();
    let mut first = stub_task("first", None);
    first.dir = Some("src".to_string());
    first.read_only = true;
    let mut second = stub_task("second", None);
    second.dir = Some("src".to_string());
    let tasks = [first, second];

    assert!(shared_dir_conflicts(&tasks).is_empty());
    assert!(validate_batch_config(&tasks, true, false).is_ok());
}

#[test]
fn find_ready_dispatches_when_individual_dep_satisfied() {
    let store = Arc::new(Store::open_memory().unwrap());
    let tasks = vec![
        stub_task("A", None),
        stub_task("B", Some(vec!["A"])),
        stub_task("C", Some(vec!["A"])),
        stub_task("D", Some(vec!["B", "C"])),
    ];
    let deps = vec![vec![], vec![0], vec![0], vec![1, 2]];
    let mut outcomes: Vec<Option<BatchTaskOutcome>> = vec![None; 4];
    let started = vec![false; 4];
    let triggered = vec![true; tasks.len()];
    let ready = find_ready_tasks(&store, &tasks, &deps, &started, &mut outcomes, &triggered).unwrap();
    assert_eq!(ready, vec![0]);

    let mut outcomes = vec![Some(BatchTaskOutcome::Done), None, None, None];
    let started = vec![true, false, false, false];
    let ready = find_ready_tasks(&store, &tasks, &deps, &started, &mut outcomes, &triggered).unwrap();
    assert_eq!(ready, vec![1, 2]);

    let mut outcomes = vec![
        Some(BatchTaskOutcome::Done),
        Some(BatchTaskOutcome::Done),
        None,
        None,
    ];
    let started = vec![true, true, true, false];
    let ready = find_ready_tasks(&store, &tasks, &deps, &started, &mut outcomes, &triggered).unwrap();
    assert!(ready.is_empty());

    let mut outcomes = vec![
        Some(BatchTaskOutcome::Done),
        Some(BatchTaskOutcome::Done),
        Some(BatchTaskOutcome::Done),
        None,
    ];
    let started = vec![true, true, true, false];
    let ready = find_ready_tasks(&store, &tasks, &deps, &started, &mut outcomes, &triggered).unwrap();
    assert_eq!(ready, vec![3]);
}

#[test]
fn find_ready_skips_tasks_with_failed_deps() {
    let store = Arc::new(Store::open_memory().unwrap());
    let tasks = vec![stub_task("A", None), stub_task("B", Some(vec!["A"]))];
    let deps = vec![vec![], vec![0]];
    let mut outcomes = vec![Some(BatchTaskOutcome::Failed), None];
    let started = vec![true, false];
    let triggered = vec![true; tasks.len()];
    let ready = find_ready_tasks(&store, &tasks, &deps, &started, &mut outcomes, &triggered).unwrap();
    assert!(ready.is_empty());
    assert_eq!(outcomes[1], Some(BatchTaskOutcome::Skipped));
}

#[test]
fn test_rate_limit_precheck_does_not_panic() {
    let temp = TempDir::new().unwrap();
    let guard = AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(crate::support::paths::aid_dir()).ok();
    rate_limit::mark_rate_limited(&AgentKind::Codex, None, "rate limit exceeded; try again at Mar 19th, 2026 2:27 PM.");
    let tasks = vec![stub_task("first", None), stub_task("second", None)];
    rate_limit_precheck(&tasks);
    drop(guard);
}

#[test]
fn read_only_with_worktree_is_rejected() {
    let mut task = stub_task("audit", None);
    task.read_only = true;
    task.worktree = Some("feat/audit".to_string());
    let err = validate_batch_config(&[task], false, false).unwrap_err();
    assert!(err.to_string().contains("read_only"));
    assert!(err.to_string().contains("worktree"));
}

#[test]
fn read_only_without_worktree_is_accepted() {
    let _temp = crate::support::test_env::TestHome::new();
    let mut task = stub_task("audit", None);
    task.read_only = true;
    validate_batch_config(&[task], false, false).unwrap();
}

#[test]
fn stopped_task_is_skipped_instead_of_failed_in_batch_results() {
    assert_eq!(batch_outcome(TaskOutcome::Stopped), BatchTaskOutcome::Skipped);
}
