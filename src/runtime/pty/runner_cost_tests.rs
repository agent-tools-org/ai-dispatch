// Completion persistence regressions using captured adapter events and typed usage scenarios.
// Exercises PTY task-row recording, component estimates, legacy fallback, and reported costs.

use super::record_completion;
use crate::agent::get_agent;
use crate::cost::test_support::*;
use crate::store::Store;
use crate::types::{AgentKind, CompletionInfo, Task, TaskEvent, TaskStatus};
use std::sync::Arc;

fn captured_event(task: &Task) -> TaskEvent {
    let raw = if task.agent == AgentKind::Codex {
        CODEX_USAGE
    } else {
        CLAUDE_USAGE
    };
    get_agent(task.agent)
        .parse_event(&task.id, raw)
        .expect("captured completion")
}

fn record(
    task: &Task,
    event: Option<&TaskEvent>,
    reported_cost: Option<f64>,
) -> (Arc<Store>, Task) {
    crate::support::paths::ensure_dirs().expect("aid dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut task = task.clone();
    task.status = TaskStatus::Running;
    store.insert_task(&task).expect("task");
    if let Some(event) = event {
        store.insert_event(event).expect("event");
    }
    let info = CompletionInfo {
        tokens: task.tokens,
        status: TaskStatus::Done,
        model: task.requested_model.clone(),
        cost_usd: reported_cost,
        exit_code: Some(0),
    };
    record_completion(
        get_agent(task.agent).as_ref(),
        &task.id,
        &store,
        task.requested_model.as_deref(),
        1_000,
        &info,
    )
    .expect("record completion");
    let stored = store
        .get_task(task.id.as_str())
        .expect("query")
        .expect("stored task");
    (store, stored)
}

#[test]
fn process_completion_stores_codex_components_and_preserves_legacy_blend() {
    let _guard = seed_prices(Some(0.125));
    let legacy = crate::cost::test_support::task(AgentKind::Claude, 2_020_000, None);
    let (_, stored) = record(&legacy, None, None);
    assert_eq!(
        stored.cost_usd,
        crate::cost::estimate_cost(2_020_000, legacy.costing_model(), AgentKind::Claude)
    );
    assert_cost(stored.cost_usd, 7.8275);
    let task = task(AgentKind::Codex, 2_020_000, None);
    let mut event = captured_event(&task);
    // Keep the captured wire bytes; supply the brief's scenario as decoded usage parameters.
    let metadata = event.metadata.as_mut().expect("usage metadata");
    metadata["tokens"] = 2_020_000.into();
    metadata["input_tokens"] = 2_000_000.into();
    metadata["uncached_input_tokens"] = 200_000.into();
    metadata["cached_input_tokens"] = 1_800_000.into();
    metadata["output_tokens"] = 20_000.into();
    let (store, stored) = record(&task, Some(&event), None);
    assert_cost(stored.cost_usd, 0.675);
    assert_cost(
        crate::cost::task_cost(&store, &stored).expect("report"),
        0.675,
    );
}

#[test]
fn process_completion_claude_without_report_prices_cache_creation_at_input_multiplier() {
    let _guard = seed_prices(Some(0.125));
    let task = task(AgentKind::Claude, 63_701, None);
    let mut event = captured_event(&task);
    event
        .metadata
        .as_mut()
        .expect("metadata")
        .as_object_mut()
        .expect("object")
        .remove("cost_usd");
    let (_, stored) = record(&task, Some(&event), None);
    assert_cost(stored.cost_usd, 0.0364344375);
}

#[test]
fn process_completion_claude_reported_cost_stays_authoritative() {
    let _guard = seed_prices(Some(0.125));
    let task = task(AgentKind::Claude, 63_701, None);
    let event = captured_event(&task);
    let (_, stored) = record(&task, Some(&event), Some(0.14359275));
    assert_eq!(stored.cost_usd, Some(0.14359275));
}

fn replay_stream(agent: &dyn crate::agent::Agent, output: &str) -> (Task, Vec<TaskEvent>) {
    let _permit = crate::support::test_subprocess::acquire();
    let home = tempfile::tempdir().expect("home");
    let _guard = crate::support::paths::AidHomeGuard::set(home.path());
    crate::support::paths::ensure_dirs().expect("aid dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut task = task(agent.kind(), 0, None);
    task.status = TaskStatus::Running;
    task.tokens = None;
    task.requested_model = Some("claude-haiku-5-5".into());
    task.custom_agent_name = agent.rate_limit_name().map(str::to_owned);
    store.insert_task(&task).expect("task");
    let input = home.path().join("captured.jsonl");
    std::fs::write(&input, output).expect("fixture");
    let mut command = std::process::Command::new("/bin/cat");
    command.arg(input);
    super::run_agent_process_with_control(
        agent,
        &command,
        &task.id,
        &store,
        &crate::support::paths::log_path(task.id.as_str()),
        None,
        task.requested_model.as_deref(),
        true,
        crate::config::timeout_policy::TimeoutPolicy::from_command(&command),
        None,
        None,
        std::time::Instant::now(),
    )
    .expect("replay through PTY runner");
    let stored = store.get_task(task.id.as_str()).expect("query").expect("stored task");
    let events = store.get_events(task.id.as_str()).expect("events");
    (stored, events)
}

#[test]
fn captured_milestone_result_stores_route_and_claude_cost_and_tokens() {
    use crate::agent::api_route::ApiRouteAgent;
    use crate::config::api_route::{ApiRoute, OverBudget};
    let route = ApiRouteAgent(ApiRoute {
        id: "claude-api-fixture".into(),
        agent: "claude".into(),
        keychain_service: "fixture".into(),
        keychain_account: None,
        monthly_budget_usd: 10.0,
        over_budget: OverBudget::Stop,
    });
    let output = include_str!("../../../tests/fixtures/streaming_completion/claude-route-unpriced.jsonl");
    for agent in [&route as &dyn crate::agent::Agent, &crate::agent::claude::ClaudeAgent] {
        let (stored, events) = replay_stream(agent, output);
        assert_eq!(stored.status, TaskStatus::Done);
        assert!(stored.cost_usd.is_some_and(|cost| (cost - 0.001305445).abs() < 1e-12));
        assert_eq!(stored.tokens, Some(20_065));
        assert!(events.iter().any(|event| event.event_kind == crate::types::EventKind::Milestone));
        assert!(events.iter().any(|event| event.metadata.as_ref().is_some_and(|metadata| {
            metadata["tokens"] == 20_065
                && metadata["cost_usd"].as_f64().is_some_and(|cost| (cost - 0.001305445).abs() < 1e-12)
        })));
    }
}

#[test]
fn cursor_milestone_result_still_stores_protocol_usage() {
    let output = concat!(
        r#"{"type":"result","result":"[MILESTONE] finished","usage":{"inputTokens":3,"#,
        r#""outputTokens":5,"cacheReadTokens":12260,"totalCostUSD":0.0123}}"#,
        "\n",
    );
    let (stored, events) = replay_stream(&crate::agent::cursor::CursorAgent, output);
    assert_eq!(stored.status, TaskStatus::Done);
    assert_eq!(stored.tokens, Some(12_268));
    assert_eq!(stored.cost_usd, Some(0.0123));
    assert!(events.iter().any(|event| event.event_kind == crate::types::EventKind::Milestone));
}

#[test]
fn milestone_line_without_completion_is_published_once() {
    let agent = crate::agent::get_agent(crate::types::AgentKind::OpenCode);
    let output = concat!(r#"{"type":"auto_compact","message":"[MILESTONE] compacted"}"#, "\n");
    let (_, events) = replay_stream(agent.as_ref(), output);
    let milestones = events
        .iter()
        .filter(|event| event.event_kind == crate::types::EventKind::Milestone)
        .count();
    assert_eq!(milestones, 1, "{events:?}");
}
