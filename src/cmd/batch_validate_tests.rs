// Tests for batch validation helpers and dependency readiness.
// Exports: none.
// Deps: super::batch_validate, crate::batch, crate::rate_limit, crate::store
use super::*;
use crate::paths::AidHomeGuard;
use crate::rate_limit;
use crate::store::Store;
use std::sync::Arc;
use tempfile::TempDir;

fn stub_task(name: &str, depends_on: Option<Vec<&str>>) -> batch::BatchTask {
    batch::BatchTask {
        id: None,
        name: Some(name.to_string()),
        agent: "codex".to_string(),
        team: None,
        prompt: "test".to_string(),
        dir: None,
        output: None,
        model: None,
        worktree: None,
        group: None,
        container: None,
        best_of: None,
        max_duration_mins: None,
        idle_timeout: None,
        verify: None,
        judge: None,
        context: None,
        checklist: None,
        skills: None,
        hooks: None,
        depends_on: depends_on.map(|values| values.into_iter().map(String::from).collect()),
        parent: None,
        context_from: None,
        fallback: None,
        scope: None,
        read_only: false,
        budget: false,
        env: None,
        env_forward: None,
        on_success: None,
        on_fail: None,
        conditional: false,
    }
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
    std::fs::create_dir_all(crate::paths::aid_dir()).ok();
    rate_limit::mark_rate_limited(
        &AgentKind::Codex,
        "rate limit exceeded; try again at Mar 19th, 2026 2:27 PM.",
    );
    let tasks = vec![stub_task("first", None), stub_task("second", None)];
    rate_limit_precheck(&tasks);
    drop(guard);
}
