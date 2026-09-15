// Async `aid run` tests split from run_tests.rs to keep files small.
// Covers dry-run dispatch and rate-limit cascade behavior.
// Deps: parent run test imports, Store, paths, tokio.
use super::{NO_SKILL_SENTINEL, RunArgs, paths, run};
use crate::store::Store;
use crate::types::{AgentKind, TaskStatus, TaskUrgency};
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

#[tokio::test]
async fn background_urgency_does_not_invoke_needs_human_child() {
    let temp = TempDir::new().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let _agents = crate::agent::DetectAgentsGuard::set(vec![]);
    let sentinel = temp.path().join("spawned");
    let script = temp.path().join("spy.sh");
    std::fs::write(&script, format!("echo invoked > \"{}\"\n", sentinel.display())).unwrap();
    let agents_dir = crate::paths::aid_dir().join("agents");
    std::fs::create_dir_all(&agents_dir).unwrap();
    std::fs::write(
        agents_dir.join("spy.toml"),
        format!(
            "[agent]\nid = \"spy\"\ndisplay_name = \"Spy\"\ncommand = \"/bin/sh\"\nfixed_args = [\"{}\"]\n",
            script.display()
        ),
    )
    .unwrap();
    crate::rate_limit::mark_rate_limited(
        &AgentKind::Custom,
        Some("spy"),
        "Error: Your credentials are invalid. Please log in again with `oz login`.",
    );
    let err = run(Arc::new(Store::open_memory().unwrap()), RunArgs {
        agent_name: "spy".to_string(),
        prompt: "Inspect the repository state".to_string(),
        declared_urgency: Some(TaskUrgency::Background),
        skills: vec![NO_SKILL_SENTINEL.to_string()],
        ..Default::default()
    })
    .await
    .expect_err("NeedsHuman hold must refuse dispatch");
    assert!(err.to_string().contains("held"), "expected a hold error, got {err}");
    assert!(!sentinel.exists(), "NeedsHuman hold must not invoke the child");
}

#[tokio::test]
async fn background_urgency_keeps_clock_hold() {
    let temp = TempDir::new().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let _agents = crate::agent::DetectAgentsGuard::set(vec![AgentKind::MiMoCode]);
    crate::rate_limit::mark_rate_limited(
        &AgentKind::MiMoCode,
        None,
        &format!("try again at {}.", crate::rate_limit::test_future_recovery_time()),
    );
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = run(store.clone(), RunArgs {
        agent_name: "mimocode".to_string(),
        prompt: "Inspect the repository state".to_string(),
        declared_urgency: Some(TaskUrgency::Background),
        dry_run: true,
        skills: vec![NO_SKILL_SENTINEL.to_string()],
        ..Default::default()
    })
    .await
    .expect("background urgency keeps a clock hold");
    let task = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(task.agent, AgentKind::MiMoCode);
    assert_eq!(task.status, TaskStatus::Skipped);
}

#[tokio::test]
async fn background_urgency_blocks_oz_needs_human_hold() {
    let temp = TempDir::new().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let _agents = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Oz]);
    crate::rate_limit::mark_rate_limited(
        &AgentKind::Oz,
        None,
        "Error: Your credentials are invalid. Please log in again with `oz login`.",
    );
    let err = run(Arc::new(Store::open_memory().unwrap()), RunArgs {
        agent_name: "oz".to_string(),
        prompt: "Inspect the repository state".to_string(),
        declared_urgency: Some(TaskUrgency::Background),
        dry_run: true,
        skills: vec![NO_SKILL_SENTINEL.to_string()],
        ..Default::default()
    })
    .await
    .expect_err("logged-out oz must not dispatch under background urgency");
    assert!(err.to_string().contains("oz is held"), "expected oz hold error, got {err}");
}
