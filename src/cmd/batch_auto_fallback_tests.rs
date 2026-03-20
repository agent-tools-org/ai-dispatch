// Tests for batch auto-fallback helpers.
// Covers one-shot retry gating and fallback agent resolution from stored tasks.
// Deps: crate::cmd::batch, crate::store::Store, crate::types.

use chrono::Local;

use super::batch::{auto_fallback_agent, should_auto_fallback, BatchTaskOutcome};
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};

fn stored_task(id: &str, agent: AgentKind) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        status: TaskStatus::Failed,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
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
        read_only: false,
        budget: false,
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
fn auto_fallback_uses_toml_specified_fallback() {
    let store = Store::open_memory().unwrap();
    store.insert_task(&stored_task("t-cursor", AgentKind::Cursor)).unwrap();
    let tasks = vec![crate::batch::BatchTask {
        id: None,
        name: None,
        agent: String::new(),
        team: None,
        prompt: String::new(),
        dir: None,
        output: None,
        model: None,
        worktree: None,
        group: None,
        container: None,
        verify: None,
        judge: None,
        best_of: None,
        max_duration_mins: None,
        context: None,
        skills: None,
        hooks: None,
        depends_on: None,
        parent: None,
        context_from: None,
        fallback: Some("opencode".to_string()),
        scope: None,
        read_only: false,
        budget: false,
        env: None,
        env_forward: None,
        on_success: None,
        on_fail: None,
        conditional: false,
    }];

    let result = auto_fallback_agent(&store, "t-cursor", &tasks, 0).unwrap();
    assert!(result.is_some());
    let (original, fallback) = result.unwrap();
    assert_eq!(original, "cursor");
    assert_eq!(fallback, AgentKind::OpenCode);
}
