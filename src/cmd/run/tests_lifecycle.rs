// Extracted lifecycle tests from tests.rs.
// Deps: parent test fixtures and module imports.
use super::*;

#[test]
fn resolve_id_conflict_blocks_running() {
    let store = Store::open_memory().unwrap();
    let mut task = make_failed_task("my-task");
    task.status = TaskStatus::Running;
    store.insert_task(&task).unwrap();
    assert!(matches!(resolve_id_conflict(&store, "my-task").unwrap(), IdConflict::Running));
}

#[test]
fn resolve_id_conflict_auto_suffixes_terminal() {
    let store = Store::open_memory().unwrap();
    store.insert_task(&make_failed_task("my-task")).unwrap();
    match resolve_id_conflict(&store, "my-task").unwrap() {
        IdConflict::AutoSuffix(new_id) => assert_eq!(new_id, "my-task-2"),
        other => panic!("expected AutoSuffix, got {:?}", std::mem::discriminant(&other)),
    }
    // Insert my-task-2, should get my-task-3 next
    store.insert_task(&make_failed_task("my-task-2")).unwrap();
    match resolve_id_conflict(&store, "my-task").unwrap() {
        IdConflict::AutoSuffix(new_id) => assert_eq!(new_id, "my-task-3"),
        other => panic!("expected AutoSuffix, got {:?}", std::mem::discriminant(&other)),
    }
}

#[test]
fn validate_dispatch_skips_dir_warning_for_non_writing_tasks() {
    assert!(validate_dispatch(&RunArgs { prompt: "Research: compare the agent options".to_string(), ..Default::default() }, &AgentKind::Codex).is_empty());
    assert!(validate_dispatch(&RunArgs { prompt: "Implement the dispatcher".to_string(), read_only: true, ..Default::default() }, &AgentKind::Codex).is_empty());
}

#[test]
fn resolve_max_duration_mins_uses_timeout_when_minutes_missing() { assert_eq!(resolve_max_duration_mins(Some(300), None), Some(5)); assert_eq!(resolve_max_duration_mins(Some(301), None), Some(6)); }

#[test]
fn resolve_max_duration_mins_preserves_explicit_minutes() { assert_eq!(resolve_max_duration_mins(Some(300), Some(2)), Some(2)); }

#[test]
fn auto_save_creates_output_for_research_task() {
    let temp = TempDir::new().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let log_path = temp.path().join("research.jsonl");
    std::fs::write(&log_path, "{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"saved output\"}\n").unwrap();
    let store = Store::open_memory().unwrap();
    let mut task = make_failed_task("t-research-save");
    task.status = TaskStatus::Done;
    task.exit_code = None;
    task.log_path = Some(log_path.display().to_string());
    store.insert_task(&task).unwrap();
    auto_save_task_output(&store, &task).unwrap();
    let output_path = crate::paths::task_dir(task.id.as_str()).join("output.md");
    assert_eq!(std::fs::read_to_string(&output_path).unwrap(), "saved output");
    assert_eq!(store.get_task(task.id.as_str()).unwrap().unwrap().output_path, Some(output_path.display().to_string()));
}

#[tokio::test]
async fn dry_run_returns_without_starting_task() {
    let temp = TempDir::new().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = run(
        store.clone(),
        RunArgs {
            agent_name: "codex".to_string(),
            prompt: "Inspect the repository state".to_string(),
            dry_run: true,
            skills: vec![NO_SKILL_SENTINEL.to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let task = store.get_task(task_id.as_str()).unwrap().unwrap();
    // Skipped, not Pending: a dry run never dispatches, and a row left pending
    // was reaped ten minutes later as a failure the agent never had.
    assert_eq!(task.status, TaskStatus::Skipped);
    assert!(task.resolved_prompt.is_some());
    assert!(task.prompt_tokens.is_some());
}

#[tokio::test]
async fn run_records_worktree_setup_failure_event() {
    let temp = TempDir::new().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-worktree-fail".to_string());

    let err = run(
        store.clone(),
        RunArgs {
            agent_name: "codex".to_string(),
            prompt: "Inspect the repository state".to_string(),
            dir: Some(temp.path().display().to_string()),
            worktree: Some("aid-worktree-fail".to_string()),
            dry_run: true,
            skills: vec![NO_SKILL_SENTINEL.to_string()],
            existing_task_id: Some(task_id.clone()),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();

    assert!(err.to_string().contains("Not a git repository"));
    assert_eq!(
        store.get_task(task_id.as_str()).unwrap().unwrap().status,
        TaskStatus::Failed
    );
    let events = store.get_events(task_id.as_str()).unwrap();
    assert!(events.iter().any(|event| {
        event.detail.contains("Failed during worktree setup: Not a git repository")
    }));
}

#[tokio::test]
async fn rate_limited_agent_without_cascade_fails_early() {
    let temp = TempDir::new().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    // No installed peers → category-aware fallback correctly returns None.
    let _agents = crate::agent::DetectAgentsGuard::set(vec![AgentKind::MiMoCode]);
    let stated = crate::rate_limit::test_future_recovery_time();
    crate::rate_limit::mark_rate_limited(
        &AgentKind::MiMoCode,
        None,
        &format!("try again at {stated}."),
    );
    let err = run(Arc::new(Store::open_memory().unwrap()), RunArgs {
        agent_name: "mimocode".to_string(),
        prompt: "Inspect the repository state".to_string(),
        dry_run: true,
        skills: vec![NO_SKILL_SENTINEL.to_string()],
        ..Default::default()
    }).await.unwrap_err();
    assert!(err.to_string().contains(&format!("mimocode is held (until {stated})")));
}

#[tokio::test]
async fn rate_limited_agent_with_cascade_proceeds() {
    let temp = TempDir::new().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    crate::rate_limit::mark_rate_limited(
        &AgentKind::Kilo,
        None,
        &format!("try again at {}.", crate::rate_limit::test_future_recovery_time()),
    );
    let task_id = run(store.clone(), RunArgs {
        agent_name: "kilo".to_string(),
        prompt: "Inspect the repository state".to_string(),
        cascade: vec!["codex".to_string()],
        dry_run: true,
        skills: vec![NO_SKILL_SENTINEL.to_string()],
        ..Default::default()
    }).await.unwrap();
    let task = store.get_task(task_id.as_str()).unwrap().unwrap();
    // Skipped, not Pending: a dry run never dispatches, and a row left pending
    // was reaped ten minutes later as a failure the agent never had.
    assert_eq!(task.status, TaskStatus::Skipped);
}
