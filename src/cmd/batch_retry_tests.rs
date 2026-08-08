// Tests for internal batch retry helpers.
// Covers rate-limit fallback selection and retry worktree bucketing.
// Deps: batch_retry private helpers, Store, rate_limit test guards.

use super::*;
use crate::cmd::run::RunArgs;
use crate::paths;
use crate::rate_limit::{clear_rate_limit, mark_rate_limited};
use crate::types::{AgentKind, TaskId, VerifyStatus};
use chrono::Local;
use tempfile::TempDir;

fn make_task(id: &str, agent: AgentKind) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent,
        custom_agent_name: None,
        prompt: "test prompt".to_string(),
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
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None, observed_model: None, attribution_source: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    }
}

fn set_repo(task: &mut Task, repo: &TempDir) {
    task.repo_path = Some(repo.path().display().to_string());
}

fn task_ids(bucket: &RetryBucket) -> Vec<String> {
    bucket.tasks.iter().map(|task| task.id.to_string()).collect()
}

fn aid_home_guard(name: &str) -> paths::AidHomeGuard {
    let temp_dir = std::env::temp_dir().join(name);
    let guard = paths::AidHomeGuard::set(&temp_dir);
    std::fs::create_dir_all(paths::aid_dir()).ok();
    guard
}

#[test]
fn retry_uses_original_when_not_rate_limited() {
    let _guard = aid_home_guard("aid-retry-fallback-test-normal");
    let repo = TempDir::new().unwrap();
    let mut task = make_task("t-001", AgentKind::Codex);
    set_repo(&mut task, &repo);
    let args = retry_task_to_run_args(&Store::open_memory().unwrap(), &task, "wg-test", None)
        .unwrap();
    assert_eq!(args.agent_name, "codex");
    clear_rate_limit(&AgentKind::Codex, None);
}

#[test]
fn retry_uses_fallback_when_rate_limited() {
    let _guard = aid_home_guard("aid-retry-fallback-test-limited");
    let _agents = crate::agent::DetectAgentsGuard::set(vec![
        AgentKind::Gemini,
        AgentKind::Qwen,
        AgentKind::Codex,
        AgentKind::Copilot,
    ]);
    mark_rate_limited(&AgentKind::Codex, None, "rate limit exceeded");
    let repo = TempDir::new().unwrap();
    let mut task = make_task("t-002", AgentKind::Codex);
    set_repo(&mut task, &repo);
    let args = retry_task_to_run_args(&Store::open_memory().unwrap(), &task, "wg-test", None)
        .unwrap();
    assert_ne!(args.agent_name, "codex", "Should use fallback when rate-limited");
    clear_rate_limit(&AgentKind::Codex, None);
}

#[test]
fn retry_uses_override_regardless_of_rate_limit() {
    let _guard = aid_home_guard("aid-retry-fallback-test-override");
    mark_rate_limited(&AgentKind::Codex, None, "rate limit exceeded");
    let repo = TempDir::new().unwrap();
    let mut task = make_task("t-003", AgentKind::Codex);
    set_repo(&mut task, &repo);
    let args =
        retry_task_to_run_args(&Store::open_memory().unwrap(), &task, "wg-test", Some("gemini"))
            .unwrap();
    assert_eq!(args.agent_name, "gemini", "Override should bypass rate limit check");
    clear_rate_limit(&AgentKind::Codex, None);
}

#[test]
fn retry_unchanged_for_unknown_agent() {
    let _guard = aid_home_guard("aid-retry-fallback-test-unknown");
    let repo = TempDir::new().unwrap();
    let mut task = make_task("t-004", AgentKind::Custom);
    set_repo(&mut task, &repo);
    let args = retry_task_to_run_args(&Store::open_memory().unwrap(), &task, "wg-test", None)
        .unwrap();
    assert_eq!(args.agent_name, "custom");
}

#[test]
fn retry_bucket_groups_shared_worktree_tasks() {
    let mut first = make_task("t-101", AgentKind::Codex);
    let mut second = make_task("t-102", AgentKind::Codex);
    let mut third = make_task("t-103", AgentKind::Codex);
    first.worktree_branch = Some("feat/gitbutler".to_string());
    second.worktree_branch = Some("feat/gitbutler".to_string());
    third.worktree_branch = Some("feat/gitbutler".to_string());

    let buckets = bucket_retry_tasks(vec![first, second, third]);

    assert_eq!(buckets.len(), 1);
    assert_eq!(
        task_ids(&buckets[0]),
        vec![
            "t-101".to_string(),
            "t-102".to_string(),
            "t-103".to_string(),
        ]
    );
}

#[test]
fn retry_bucket_separates_distinct_worktrees() {
    let mut first = make_task("t-201", AgentKind::Codex);
    let mut second = make_task("t-202", AgentKind::Codex);
    first.worktree_branch = Some("feat/a".to_string());
    second.worktree_branch = Some("feat/b".to_string());

    let buckets = bucket_retry_tasks(vec![first, second]);

    assert_eq!(buckets.len(), 2);
    assert_eq!(task_ids(&buckets[0]), vec!["t-201".to_string()]);
    assert_eq!(task_ids(&buckets[1]), vec!["t-202".to_string()]);
}

#[test]
fn retry_bucket_treats_none_worktree_as_unique() {
    let first = make_task("t-301", AgentKind::Codex);
    let second = make_task("t-302", AgentKind::Codex);
    let third = make_task("t-303", AgentKind::Codex);

    let buckets = bucket_retry_tasks(vec![first, second, third]);

    assert_eq!(buckets.len(), 3);
    assert_eq!(task_ids(&buckets[0]), vec!["t-301".to_string()]);
    assert_eq!(task_ids(&buckets[1]), vec!["t-302".to_string()]);
    assert_eq!(task_ids(&buckets[2]), vec!["t-303".to_string()]);
}

/// `aid batch retry --agent <other>` must drop session_id from the saved
/// dispatch args. Before this fix, the model was cleared but session_id from
/// the saved RunArgs was left intact and handed to the new CLI.
#[test]
fn batch_retry_agent_override_clears_session_id() {
    let _guard = aid_home_guard("aid-batch-retry-session-clear");
    let repo = TempDir::new().unwrap();
    let mut task = make_task("t-session-override", AgentKind::Codex);
    set_repo(&mut task, &repo);
    let store = Store::open_memory().unwrap();
    // Simulate saved dispatch args that contain a session token and model from
    // the original codex run.
    let saved = RunArgs {
        agent_name: "codex".to_string(),
        prompt: task.prompt.clone(),
        session_id: Some("codex-session-token".to_string()),
        model: Some("gpt-5".to_string()),
        ..Default::default()
    };
    store
        .update_task_dispatch_args(task.id.as_str(), &saved.dispatch_args_json().unwrap())
        .unwrap();

    let args = retry_task_to_run_args(&store, &task, "wg-test", Some("gemini")).unwrap();

    assert_eq!(args.agent_name, "gemini");
    assert!(
        args.session_id.is_none(),
        "session_id from saved dispatch must be cleared on agent change, got {:?}",
        args.session_id
    );
    assert!(args.model.is_none(), "model must be cleared on agent change");
}
