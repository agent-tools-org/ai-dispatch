// Shared captured completion bytes and typed usage setup for component cost tests.
// Depends on the cost test seams, isolated aid homes, and task domain types.

use super::components::TokenUsage;
use super::{
    clear_feed_for_tests,
    price_feed::{Feed, FeedModel},
    set_feed_for_tests,
};
use crate::paths::AidHomeGuard;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};

pub(crate) const CODEX_USAGE: &str = concat!(
    r#"{"type":"turn.completed","usage":{"input_tokens":232452,"cached_input_tokens":211968,"#,
    r#""output_tokens":5988}}"#,
);

pub(crate) const CLAUDE_USAGE: &str = concat!(
    r#"{"type":"result","subtype":"success","result":"Hello!","total_cost_usd":0.14359275,"s"#,
    r#"ession_id":"session-1","usage":{"input_tokens":4,"cache_creation_input_tokens":18821,"#,
    r#""cache_read_input_tokens":44733,"output_tokens":143},"modelUsage":{"claude-opus-4-6[1"#,
    r#"m]":{"inputTokens":4}}}"#,
);

pub(crate) fn seed_prices(cached: Option<f64>) -> (tempfile::TempDir, AidHomeGuard) {
    let dir = tempfile::tempdir().expect("tempdir");
    let guard = AidHomeGuard::set(dir.path());
    clear_feed_for_tests();
    set_feed_for_tests(Feed {
        built_at: "2026-10-08T00:00:00Z".to_string(),
        age_seconds: Some(0),
        stale: Some(false),
        count: Some(1),
        models: vec![FeedModel {
            id: "component-test-model".to_string(),
            aliases: vec![],
            input_per_mtok: 1.25,
            output_per_mtok: 10.0,
            cached_input_per_mtok: cached,
            context_length: None,
            source: None,
        }],
    });
    (dir, guard)
}

pub(crate) fn task(agent: AgentKind, tokens: i64, stored_cost: Option<f64>) -> Task {
    Task {
        id: TaskId("t-components".to_string()),
        agent,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        project_id: None,
        worktree_path: None,
        effective_dir: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: Some(tokens),
        prompt_tokens: None,
        duration_ms: None,
        requested_model: Some("component-test-model".to_string()),
        observed_model: None,
        attribution_source: None,
        cost_usd: stored_cost,
        exit_code: None,
        created_at: chrono::Local::now(),
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

pub(crate) fn sample_usage() -> TokenUsage {
    TokenUsage {
        uncached_input: 200_000,
        cached_input: 1_800_000,
        output: 20_000,
        cache_creation: 0,
    }
}

pub(crate) fn assert_cost(actual: Option<f64>, expected: f64) {
    let actual = actual.expect("known estimate");
    assert!(
        (actual - expected).abs() < 1e-9,
        "cost {actual} != expected {expected}"
    );
}
