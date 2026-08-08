// Async `aid run` tests split from run_tests.rs to keep files small.
// Covers dry-run dispatch and rate-limit cascade behavior.
// Deps: parent run test imports, Store, paths, tokio.
use super::{NO_SKILL_SENTINEL, RunArgs, paths, run};
use crate::store::Store;
use crate::types::{AgentKind, TaskStatus};
use std::sync::Arc;
use tempfile::TempDir;

#[tokio::test]
async fn dry_run_returns_without_starting_task() {
    let temp = TempDir::new().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = run(store.clone(), RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Inspect the repository state".to_string(),
        dry_run: true,
        skills: vec![NO_SKILL_SENTINEL.to_string()],
        ..Default::default()
    }).await.unwrap();
    let task = store.get_task(task_id.as_str()).unwrap().unwrap();
    // Skipped, not Pending: a dry run never dispatches, and a row left pending
    // was reaped ten minutes later as a failure the agent never had.
    assert_eq!(task.status, TaskStatus::Skipped);
    assert!(task.resolved_prompt.is_some());
    assert!(task.prompt_tokens.is_some());
}

#[tokio::test]
async fn rate_limited_agent_without_cascade_fails_early() {
    let temp = TempDir::new().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    // No installed peers → category-aware fallback correctly returns None.
    let _agents = crate::agent::DetectAgentsGuard::set(vec![AgentKind::MiMoCode]);
    crate::rate_limit::mark_rate_limited(&AgentKind::MiMoCode, None, "try again at Mar 21st, 2099 2:27 PM.");
    let err = run(Arc::new(Store::open_memory().unwrap()), RunArgs {
        agent_name: "mimocode".to_string(),
        prompt: "Inspect the repository state".to_string(),
        dry_run: true,
        skills: vec![NO_SKILL_SENTINEL.to_string()],
        ..Default::default()
    }).await.unwrap_err();
    assert!(err.to_string().contains("mimocode is held (until Mar 21st, 2099 2:27 PM)"));
}

#[tokio::test]
async fn rate_limited_agent_with_cascade_proceeds() {
    let temp = TempDir::new().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    crate::rate_limit::mark_rate_limited(&AgentKind::Kilo, None, "try again at Mar 21st, 2099 2:27 PM.");
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
