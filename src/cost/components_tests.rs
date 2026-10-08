// Component cost regressions using existing captured Codex and Claude completion bytes.
// Covers feed cache pricing, fallback rates, persisted estimates, and reported cost authority.

use super::*;
use crate::agent::{Agent, claude::ClaudeAgent, codex::CodexAgent};
use crate::cost::test_support::*;
use crate::types::TaskId;

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
fn cost_components_claude_creation_uses_input_multiplier_when_rate_is_unknown() {
    let _guard = seed_prices(Some(0.125));
    let event = ClaudeAgent
        .parse_event(&TaskId("t-components".to_string()), CLAUDE_USAGE)
        .expect("captured completion");
    let metadata = event.metadata.as_ref().expect("metadata");
    // Exercise the estimate path on captured usage rather than fabricating a wire envelope.
    let usage = usage_from_metadata(metadata, AgentKind::Claude).expect("usage");
    assert_cost(
        estimate_usage_cost(usage, Some("component-test-model"), AgentKind::Claude),
        (4.0 * 1.25 + 44_733.0 * 0.125 + 143.0 * 10.0 + 18_821.0 * 1.25 * 1.25) / 1_000_000.0,
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

#[test]
fn cost_components_explicit_creation_price_wins_over_input_multiplier() {
    let _guard = seed_prices(Some(0.125));
    crate::cost::TEST_PRICING_OVERRIDES.with(|cell| {
        *cell.borrow_mut() = Some(std::collections::HashMap::from([(
            (AgentKind::Claude, "component-test-model".to_string()),
            crate::cost::ModelPricing {
                input_per_m: 1.25,
                output_per_m: 10.0,
                cached_input_per_m: Some(0.125),
                cache_creation_per_m: Some(3.0),
            },
        )]));
    });
    let usage = TokenUsage {
        uncached_input: 0,
        cached_input: 0,
        output: 0,
        cache_creation: 1_000_000,
    };
    assert_cost(
        estimate_usage_cost(usage, Some("component-test-model"), AgentKind::Claude),
        3.0,
    );
}

#[test]
fn watcher_completion_components_set_cost_and_replace_previous_totals() {
    use crate::types::{CompletionInfo, TaskStatus};
    use crate::watcher::{
        StreamLineContext, SyntheticMilestoneTracker, handle_streaming_line_with_session,
    };
    let _guard = seed_prices(Some(0.125));
    let store = std::sync::Arc::new(Store::open_memory().expect("store"));
    let mut task = task(AgentKind::Codex, 238_440, None);
    task.observed_model = Some("previous-run-model".to_string());
    store.insert_task(&task).expect("task");
    let mut info = CompletionInfo {
        tokens: None,
        model: None,
        status: TaskStatus::Done,
        cost_usd: None,
        exit_code: None,
    };
    let (mut count, mut saved) = (0, false);
    let mut tracker = SyntheticMilestoneTracker::new();
    for _ in 0..2 {
        handle_streaming_line_with_session(
            StreamLineContext {
                agent: &CodexAgent,
                task_id: &task.id,
                store: &store,
                workgroup_id: None,
                synthetic_tracker: &mut tracker,
            },
            &mut info,
            &mut count,
            CODEX_USAGE,
            &mut saved,
        )
        .expect("streaming completion");
        assert_eq!(info.tokens, Some(238_440));
        assert_cost(info.cost_usd, 0.111981);
    }
}

#[test]
fn cost_completion_reprices_stream_estimate_using_final_model() {
    let _guard = seed_prices(Some(0.125));
    let store = Store::open_memory().expect("store");
    let task = task(AgentKind::Codex, 238_440, None);
    let event = CodexAgent
        .parse_event(&task.id, CODEX_USAGE)
        .expect("capture");
    store.insert_task(&task).expect("task");
    store.insert_event(&event).expect("event");
    let info = crate::types::CompletionInfo {
        tokens: task.tokens,
        model: None,
        status: crate::types::TaskStatus::Done,
        cost_usd: Some(0.111981),
        exit_code: Some(0),
    };
    assert_eq!(
        completion_cost(&store, &task.id, &info, Some("missing"), AgentKind::Codex)
            .expect("unknown final model"),
        None
    );
}
