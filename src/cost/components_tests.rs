// Component cost regressions using existing captured Codex and Claude completion bytes.
// Covers feed cache pricing, fallback rates, persisted estimates, and reported cost authority.

use super::*;
use crate::agent::{Agent, claude::ClaudeAgent, codex::CodexAgent};
use crate::cost::{
    clear_feed_for_tests,
    price_feed::{Feed, FeedModel},
    set_feed_for_tests,
};
use crate::paths::AidHomeGuard;
use crate::types::{TaskId, TaskStatus, VerifyStatus};

const CODEX_USAGE: &str = concat!(
    r#"{"type":"turn.completed","usage":{"input_tokens":232452,"cached_input_tokens":211968,"#,
    r#""output_tokens":5988}}"#,
);

const CLAUDE_USAGE: &str = concat!(
    r#"{"type":"result","subtype":"success","result":"Hello!","total_cost_usd":0.14359275,"s"#,
    r#"ession_id":"session-1","usage":{"input_tokens":4,"cache_creation_input_tokens":18821,"#,
    r#""cache_read_input_tokens":44733,"output_tokens":143},"modelUsage":{"claude-opus-4-6[1"#,
    r#"m]":{"inputTokens":4}}}"#,
);

fn seed_prices(cached: Option<f64>) -> (tempfile::TempDir, AidHomeGuard) {
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

fn task(agent: AgentKind, tokens: i64, stored_cost: Option<f64>) -> Task {
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

fn sample_usage() -> TokenUsage {
    TokenUsage {
        uncached_input: 200_000,
        cached_input: 1_800_000,
        output: 20_000,
        cache_creation: 0,
    }
}

fn assert_cost(actual: Option<f64>, expected: f64) {
    let actual = actual.expect("known estimate");
    assert!(
        (actual - expected).abs() < 1e-9,
        "cost {actual} != expected {expected}"
    );
}

#[test]
fn cost_components_price_cached_reads_separately() {
    let _guard = seed_prices(Some(0.125));
    assert_cost(
        estimate_usage_cost(
            sample_usage(),
            Some("component-test-model"),
            AgentKind::Codex,
        ),
        0.675,
    );
}

#[test]
fn cost_components_unknown_cached_price_uses_blend_only_for_cached_part() {
    let _guard = seed_prices(None);
    assert_cost(
        estimate_usage_cost(
            sample_usage(),
            Some("component-test-model"),
            AgentKind::Codex,
        ),
        0.25 + 0.20 + 1.8 * 3.875,
    );
}

#[test]
fn cost_components_unknown_model_is_unknown_and_free_cache_is_zero() {
    let _guard = seed_prices(Some(0.125));
    assert_eq!(
        estimate_usage_cost(sample_usage(), Some("missing"), AgentKind::Codex),
        None
    );
    assert_eq!(
        estimate_usage_cost(sample_usage(), None, AgentKind::Cursor),
        Some(0.0)
    );
}

#[test]
fn cost_components_output_only_and_input_only_use_their_own_rates() {
    let _guard = seed_prices(Some(0.125));
    let usage = TokenUsage {
        uncached_input: 1_000_000,
        cached_input: 0,
        output: 0,
        cache_creation: 0,
    };
    assert_cost(
        estimate_usage_cost(usage, Some("component-test-model"), AgentKind::Codex),
        1.25,
    );
    let usage = TokenUsage {
        uncached_input: 0,
        output: 1_000_000,
        ..usage
    };
    assert_cost(
        estimate_usage_cost(usage, Some("component-test-model"), AgentKind::Codex),
        10.0,
    );
}

#[test]
fn cost_components_codex_capture_replaces_persisted_blended_estimate() {
    let _guard = seed_prices(Some(0.125));
    let store = Store::open_memory().expect("store");
    let task = task(AgentKind::Codex, 238_440, Some(0.923955));
    let event = CodexAgent
        .parse_event(&task.id, CODEX_USAGE)
        .expect("captured completion");
    assert_eq!(
        event.metadata.as_ref().expect("metadata")["uncached_input_tokens"],
        20_484
    );
    store.insert_task(&task).expect("task");
    store.insert_event(&event).expect("event");
    assert_cost(task_cost(&store, &task).expect("estimate"), 0.111981);
}

#[test]
fn cost_components_claude_reported_cost_stays_authoritative() {
    let _guard = seed_prices(Some(0.125));
    let store = Store::open_memory().expect("store");
    let task = task(AgentKind::Claude, 63_701, Some(0.14359275));
    let event = ClaudeAgent
        .parse_event(&task.id, CLAUDE_USAGE)
        .expect("captured completion");
    assert_eq!(
        event.metadata.as_ref().expect("metadata")["uncached_input_tokens"],
        4
    );
    store.insert_task(&task).expect("task");
    store.insert_event(&event).expect("event");
    assert_eq!(
        task_cost(&store, &task).expect("estimate"),
        Some(0.14359275)
    );
}

#[test]
fn cost_components_claude_creation_uses_blend_when_rate_is_unknown() {
    let _guard = seed_prices(Some(0.125));
    let event = ClaudeAgent
        .parse_event(&TaskId("t-components".to_string()), CLAUDE_USAGE)
        .expect("captured completion");
    let metadata = event.metadata.as_ref().expect("metadata");
    // Exercise the estimate path on captured usage rather than fabricating a wire envelope.
    let usage = usage_from_metadata(metadata, AgentKind::Claude).expect("usage");
    assert_cost(
        estimate_usage_cost(usage, Some("component-test-model"), AgentKind::Claude),
        (4.0 * 1.25 + 44_733.0 * 0.125 + 143.0 * 10.0 + 18_821.0 * 3.875) / 1_000_000.0,
    );
}

#[test]
fn cost_components_legacy_totals_keep_blend_and_missing_prices_stay_unknown() {
    let _guard = seed_prices(None);
    let store = Store::open_memory().expect("store");
    let mut task = task(AgentKind::Codex, 1_000_000, None);
    assert_cost(task_cost(&store, &task).expect("estimate"), 3.875);
    task.requested_model = Some("missing".to_string());
    assert_eq!(task_cost(&store, &task).expect("unknown"), None);
    task.cost_usd = Some(0.75);
    assert_eq!(task_cost(&store, &task).expect("stored cost"), Some(0.75));
}

#[test]
fn cost_components_catalog_precedence_keeps_unknown_cached_rate() {
    let _guard = seed_prices(Some(0.125));
    let usage = sample_usage();
    assert_cost(
        estimate_usage_cost(usage, Some("gpt-5.6-sol"), AgentKind::Codex),
        0.2 * 2.5 + 1.8 * (0.7 * 2.5 + 0.3 * 15.0) + 0.02 * 15.0,
    );
}
