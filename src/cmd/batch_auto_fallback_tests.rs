// Tests for batch auto-fallback helpers.
// Covers one-shot retry gating and fallback agent resolution from stored tasks.
// Deps: crate::cmd::batch, crate::store::Store, crate::types.

use tempfile::TempDir;

use super::batch::{
    BatchTaskOutcome, auto_fallback_args, pre_dispatch_fallback_choice, should_auto_fallback,
};
use crate::support::paths::AidHomeGuard;
use crate::routing::rate_limit::{clear_rate_limit, mark_rate_limited};
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus};

fn stored_task(id: &str, agent: AgentKind) -> Task {
    Task {
        agent,
        ..crate::types::Task::test_fixture(TaskId(id.to_string()), TaskStatus::Failed)
    }
}

fn isolated_rate_limit_home() -> (TempDir, AidHomeGuard) {
    let temp_dir = TempDir::new().unwrap();
    let guard = AidHomeGuard::set(temp_dir.path());
    std::fs::create_dir_all(crate::support::paths::aid_dir()).unwrap();
    (temp_dir, guard)
}

fn dispatch_agent_name(agent_name: &str, fallback: Option<&str>) -> String {
    match pre_dispatch_fallback_choice(agent_name, fallback).unwrap() {
        Some((fallback_agent, _)) => fallback_agent,
        None => agent_name.to_string(),
    }
}

#[test]
fn should_auto_fallback_only_once_for_failed_tasks() {
    assert!(should_auto_fallback(true, false, BatchTaskOutcome::Failed));
    assert!(!should_auto_fallback(true, true, BatchTaskOutcome::Failed));
    assert!(!should_auto_fallback(true, false, BatchTaskOutcome::Done));
    assert!(!should_auto_fallback(false, false, BatchTaskOutcome::Failed));
}

#[test]
fn auto_fallback_args_returns_none_when_no_usable_peer() {
    let (_temp, _guard) = isolated_rate_limit_home();
    let store = Store::open_memory().unwrap();
    // Only the exhausted agent is installed — category-aware fallback must not invent peers.
    let _agents = crate::agent::DetectAgentsGuard::set(vec![AgentKind::MiMoCode]);
    store.insert_task(&stored_task("t-mimocode", AgentKind::MiMoCode)).unwrap();

    assert!(
        auto_fallback_args(&store, "t-mimocode", &[], 0)
            .unwrap()
            .is_none()
    );
}

#[test]
fn pre_dispatch_uses_fallback_when_agent_is_rate_limited() {
    let (_temp, _guard) = isolated_rate_limit_home();
    mark_rate_limited(
        &AgentKind::Codex,
        None,
        &format!(
            "You've hit your usage limit. try again at {}.",
            crate::routing::rate_limit::test_future_recovery_time()
        ),
    );

    let choice = pre_dispatch_fallback_choice("codex", Some("opencode,cursor"))
        .unwrap()
        .expect("fallback choice");

    assert_eq!(dispatch_agent_name("codex", Some("opencode,cursor")), "opencode");
    assert_eq!(choice.0, "opencode");
    assert_eq!(choice.1, vec!["cursor".to_string()]);

    clear_rate_limit(&AgentKind::Codex, None);
}

#[test]
fn pre_dispatch_keeps_original_when_no_fallback_is_available() {
    let (_temp, _guard) = isolated_rate_limit_home();
    mark_rate_limited(
        &AgentKind::Codex,
        None,
        &format!(
            "You've hit your usage limit. try again at {}.",
            crate::routing::rate_limit::test_future_recovery_time()
        ),
    );

    assert_eq!(dispatch_agent_name("codex", None), "codex");
    assert!(pre_dispatch_fallback_choice("codex", None).unwrap().is_none());

    clear_rate_limit(&AgentKind::Codex, None);
}

#[test]
fn auto_fallback_skips_rate_limited_toml_fallbacks() {
    let (_temp, _guard) = isolated_rate_limit_home();
    mark_rate_limited(
        &AgentKind::OpenCode,
        None,
        "Insufficient balance. Manage your billing here",
    );

    let store = Store::open_memory().unwrap();
    store.insert_task(&stored_task("t-codex", AgentKind::Codex)).unwrap();
    let tasks = vec![batch_task_with_fallback("opencode,cursor")];

    let result = auto_fallback_args(&store, "t-codex", &tasks, 0).unwrap();
    assert!(result.is_some());
    let (original, fallback) = result.unwrap();
    assert_eq!(original, "codex");
    assert_eq!(fallback.agent_name, "cursor");

    clear_rate_limit(&AgentKind::OpenCode, None);
}

fn write_custom_agent(name: &str) {
    let agents_dir = crate::support::paths::aid_dir().join("agents");
    std::fs::create_dir_all(&agents_dir).unwrap();
    std::fs::write(
        agents_dir.join(format!("{name}.toml")),
        format!("[agent]\nid = \"{name}\"\ndisplay_name = \"{name}\"\ncommand = \"{name}\"\n"),
    )
    .unwrap();
}

fn batch_task_with_fallback(fallback: &str) -> crate::batch::BatchTask {
    crate::batch::BatchTask {
        agent: String::new(),
        prompt: String::new(),
        fallback: Some(fallback.to_string()),
        ..Default::default()
    }
}

/// Custom agents named in batch fallback must be selected — same rule as
/// `aid run` cascade parsing, not silently dropped by AgentKind::parse_str.
#[test]
fn pre_dispatch_uses_custom_agent_in_fallback() {
    let (_temp, _guard) = isolated_rate_limit_home();
    write_custom_agent("glm5");
    mark_rate_limited(
        &AgentKind::Codex,
        None,
        &format!(
            "You've hit your usage limit. try again at {}.",
            crate::routing::rate_limit::test_future_recovery_time()
        ),
    );

    let choice = pre_dispatch_fallback_choice("codex", Some("glm5,cursor"))
        .unwrap()
        .expect("custom fallback");

    assert_eq!(choice.0, "glm5");
    assert_eq!(choice.1, vec!["cursor".to_string()]);

    clear_rate_limit(&AgentKind::Codex, None);
}

/// An unresolvable batch fallback entry must surface as an error, never be
/// skipped the way filter_map(AgentKind::parse_str) used to.
#[test]
fn pre_dispatch_unknown_fallback_is_an_error() {
    let (_temp, _guard) = isolated_rate_limit_home();
    mark_rate_limited(
        &AgentKind::Codex,
        None,
        &format!(
            "You've hit your usage limit. try again at {}.",
            crate::routing::rate_limit::test_future_recovery_time()
        ),
    );

    let err = pre_dispatch_fallback_choice("codex", Some("not-a-real-agent"))
        .expect_err("unknown cascade agent must error");
    assert!(
        err.to_string().contains("not-a-real-agent"),
        "error must name the unknown agent: {err}"
    );

    clear_rate_limit(&AgentKind::Codex, None);
}

#[test]
fn auto_fallback_args_selects_custom_toml_fallback() {
    let (_temp, _guard) = isolated_rate_limit_home();
    write_custom_agent("glm5");

    let store = Store::open_memory().unwrap();
    store.insert_task(&stored_task("t-codex", AgentKind::Codex)).unwrap();
    let tasks = vec![batch_task_with_fallback("glm5")];

    let result = auto_fallback_args(&store, "t-codex", &tasks, 0).unwrap();
    let (original, fallback) = result.expect("custom fallback");
    assert_eq!(original, "codex");
    assert_eq!(fallback.agent_name, "glm5");
}
