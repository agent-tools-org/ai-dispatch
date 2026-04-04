// Tests for batch auto-fallback helpers.
// Covers one-shot retry gating and fallback agent resolution from stored tasks.
// Deps: crate::cmd::batch, crate::store::Store, crate::types.

use chrono::Local;
use tempfile::TempDir;

use super::batch::{
    auto_fallback_agent,
    pre_dispatch_fallback_choice,
    should_auto_fallback,
    BatchTaskOutcome,
};
use crate::paths::AidHomeGuard;
use crate::rate_limit::{clear_rate_limit, mark_rate_limited};
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};

fn stored_task(id: &str, agent: AgentKind) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Failed,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        model: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
    }
}

fn isolated_rate_limit_home() -> (TempDir, AidHomeGuard) {
    let temp_dir = TempDir::new().unwrap();
    let guard = AidHomeGuard::set(temp_dir.path());
    std::fs::create_dir_all(crate::paths::aid_dir()).unwrap();
    (temp_dir, guard)
}

fn dispatch_agent_name(agent_name: &str, fallback: Option<&str>) -> String {
    match pre_dispatch_fallback_choice(agent_name, fallback) {
        Some((fallback_agent, _)) => fallback_agent.as_str().to_string(),
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
fn auto_fallback_agent_returns_none_when_chain_ends() {
    let store = Store::open_memory().unwrap();
    store.insert_task(&stored_task("t-kilo", AgentKind::Kilo)).unwrap();

    assert!(auto_fallback_agent(&store, "t-kilo", &[], 0).unwrap().is_none());
}

#[test]
fn pre_dispatch_uses_fallback_when_agent_is_rate_limited() {
    let (_temp, _guard) = isolated_rate_limit_home();
    mark_rate_limited(&AgentKind::Codex, "rate limit exceeded");

    let choice = pre_dispatch_fallback_choice("codex", Some("opencode,cursor")).unwrap();

    assert_eq!(dispatch_agent_name("codex", Some("opencode,cursor")), "opencode");
    assert_eq!(choice.0, AgentKind::OpenCode);
    assert_eq!(choice.1, vec!["cursor".to_string()]);

    clear_rate_limit(&AgentKind::Codex);
}

#[test]
fn pre_dispatch_keeps_original_when_no_fallback_is_available() {
    let (_temp, _guard) = isolated_rate_limit_home();
    mark_rate_limited(&AgentKind::Codex, "rate limit exceeded");

    assert_eq!(dispatch_agent_name("codex", None), "codex");
    assert!(pre_dispatch_fallback_choice("codex", None).is_none());

    clear_rate_limit(&AgentKind::Codex);
}

#[test]
fn auto_fallback_skips_rate_limited_toml_fallbacks() {
    let (_temp, _guard) = isolated_rate_limit_home();
    mark_rate_limited(&AgentKind::OpenCode, "rate limit exceeded");

    let store = Store::open_memory().unwrap();
    store.insert_task(&stored_task("t-codex", AgentKind::Codex)).unwrap();
    let tasks = vec![crate::batch::BatchTask {
        id: None,
        name: None,
        agent: String::new(),
        team: None,
        prompt: String::new(),
        dir: None,
        output: None,
        result_file: None,
        model: None,
        worktree: None,
        group: None,
        container: None,
        verify: None,
        judge: None,
        peer_review: None,
        best_of: None,
        max_duration_mins: None,
        retry: None,
        idle_timeout: None,
        metric: None,
        context: None,
        checklist: None,
        skills: None,
        on_done: None,
        hooks: None,
        depends_on: None,
        parent: None,
        context_from: None,
        fallback: Some("opencode,cursor".to_string()),
        scope: None,
        read_only: false,
        sandbox: false,
        no_skill: false,
        budget: false,
        env: None,
        env_forward: None,
        on_success: None,
        on_fail: None,
        conditional: false,
    }];

    let result = auto_fallback_agent(&store, "t-codex", &tasks, 0).unwrap();
    assert!(result.is_some());
    let (original, fallback) = result.unwrap();
    assert_eq!(original, "codex");
    assert_eq!(fallback, AgentKind::Cursor);

    clear_rate_limit(&AgentKind::OpenCode);
}
