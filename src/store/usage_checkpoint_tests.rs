// Usage checkpoint regressions through stream ingestion and Store settlement.
// Covers durable recovery, cumulative and delta accounting, unknown usage and terminal races.
use super::*;
use crate::agent::{Agent, get_agent};
use crate::runtime::watcher::{
    StreamLineContext, SyntheticMilestoneTracker, handle_streaming_line_with_session,
};
use crate::store::TaskCompletionUpdate;
use crate::types::*;
use std::sync::Arc;

const RESULTS: &str =
    include_str!("../../tests/fixtures/streaming_completion/claude-cumulative-results.jsonl");

fn setup(store: &Store, kind: AgentKind) -> TaskId {
    let id = TaskId("t-checkpoint".into());
    store
        .db()
        .execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at)
         VALUES (?1, ?2, '', 'running', '2026-10-20T00:00:00Z')",
            params![id.as_str(), kind.as_str()],
        )
        .expect("task");
    id
}

fn info() -> CompletionInfo {
    CompletionInfo {
        tokens: None,
        cost_usd: None,
        model: None,
        status: TaskStatus::Done,
        exit_code: None,
    }
}

fn ingest(store: &Arc<Store>, id: &TaskId, agent: &dyn Agent, lines: &str) -> CompletionInfo {
    let mut info = info();
    let mut tracker = SyntheticMilestoneTracker::new();
    let mut count = 0;
    let mut saved = false;
    for line in lines.lines() {
        handle_streaming_line_with_session(
            StreamLineContext {
                agent,
                task_id: id,
                store,
                workgroup_id: None,
                synthetic_tracker: &mut tracker,
            },
            &mut info,
            &mut count,
            line,
            &mut saved,
        )
        .expect("ingest");
    }
    info
}

fn complete(
    store: &Store,
    id: &TaskId,
    status: TaskStatus,
    tokens: Option<i64>,
    cost: Option<f64>,
) {
    assert!(
        store
            .update_task_completion(TaskCompletionUpdate {
                id: id.as_str(),
                status,
                tokens,
                duration_ms: 100,
                observed_model: None,
                attribution_source: None,
                cost_usd: cost,
                exit_code: None,
            })
            .expect("complete")
    );
}

fn assert_usage(store: &Store, id: &TaskId, tokens: i64, cost: f64) {
    let task = store.get_task(id.as_str()).expect("query").expect("task");
    assert_eq!(task.tokens, Some(tokens));
    assert!((task.cost_usd.expect("cost") - cost).abs() < 1e-12);
}

#[test]
fn claude_checkpoint_survives_worker_loss_and_database_reopen() {
    let dir = tempfile::tempdir().expect("dir");
    let _home = crate::support::paths::AidHomeGuard::set(dir.path());
    crate::support::paths::ensure_dirs().expect("dirs");
    let db = dir.path().join("aid.db");
    let store = Arc::new(Store::open(&db).expect("store"));
    let id = setup(&store, AgentKind::Claude);
    let info = ingest(&store, &id, &crate::agent::claude::ClaudeAgent, RESULTS);
    assert_eq!(info.tokens, Some(10_633_915));
    let task = store.get_task(id.as_str()).expect("query").expect("task");
    assert_eq!(
        task.tokens, None,
        "checkpoint is separate from task settlement"
    );
    drop(store);
    let store = Store::open(&db).expect("reopen");
    assert!(
        crate::runtime::background::record_failure(
            &store,
            id.as_str(),
            "worker disappeared",
            "worker disappeared",
        )
        .expect("reaper failure")
    );
    assert_usage(&store, &id, 10_633_915, 4.5095448);
}

#[test]
fn normal_and_worker_failure_settlement_recover_checkpoint() {
    for status in [TaskStatus::Done, TaskStatus::Failed] {
        let store = Arc::new(Store::open_memory().expect("store"));
        let id = setup(&store, AgentKind::Claude);
        ingest(&store, &id, &crate::agent::claude::ClaudeAgent, RESULTS);
        complete(&store, &id, status, None, None);
        assert_usage(&store, &id, 10_633_915, 4.5095448);
    }
}

#[test]
fn stopped_task_keeps_checkpoint_and_rejects_late_completion() {
    let dir = tempfile::tempdir().expect("dir");
    let _home = crate::support::paths::AidHomeGuard::set(dir.path());
    crate::support::paths::ensure_dirs().expect("dirs");
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = setup(&store, AgentKind::Claude);
    let agent = crate::agent::claude::ClaudeAgent;
    ingest(&store, &id, &agent, RESULTS);
    crate::cmd::stop::kill(&store, id.as_str()).expect("stop");
    let events = store.get_events(id.as_str()).expect("events").len();
    ingest(
        &store,
        &id,
        &agent,
        r#"{"type":"result","usage":{"input_tokens":1},"total_cost_usd":9}"#,
    );
    assert_eq!(store.get_events(id.as_str()).expect("events").len(), events);
    assert_usage(&store, &id, 10_633_915, 4.5095448);
}

#[test]
fn claude_repeated_cumulative_results_are_not_added() {
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = setup(&store, AgentKind::Claude);
    let repeated = format!("{RESULTS}{RESULTS}");
    ingest(&store, &id, &crate::agent::claude::ClaudeAgent, &repeated);
    complete(&store, &id, TaskStatus::Done, None, None);
    assert_usage(&store, &id, 10_633_915, 4.5095448);
}

#[test]
fn missing_usage_remains_unknown_on_all_settlement_paths() {
    for status in [TaskStatus::Done, TaskStatus::Failed, TaskStatus::Stopped] {
        for lines in [
            "",
            r#"{"type":"result","result":"finished"}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"partial"}],"usage":{"input_tokens":42}}}"#,
        ] {
            let store = Arc::new(Store::open_memory().expect("store"));
            let id = setup(&store, AgentKind::Claude);
            ingest(&store, &id, &crate::agent::claude::ClaudeAgent, lines);
            if status == TaskStatus::Stopped {
                store.update_task_status(id.as_str(), status).expect("stop");
            } else {
                complete(&store, &id, status, None, None);
            }
            let task = store.get_task(id.as_str()).expect("query").expect("task");
            assert_eq!(task.tokens, None);
            assert_eq!(task.cost_usd, None);
        }
    }
}

#[test]
fn opencode_checkpoint_preserves_delta_step_sums() {
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = setup(&store, AgentKind::OpenCode);
    let lines = include_str!("../../tests/fixtures/streaming_completion/opencode-multi-step.jsonl");
    ingest(&store, &id, get_agent(AgentKind::OpenCode).as_ref(), lines);
    store.fail_active_execution(id.as_str()).expect("failure");
    assert_usage(&store, &id, 1_498_073, 0.10654006);
}

#[test]
fn codex_checkpoint_preserves_component_pricing() {
    let _prices = crate::cost::test_support::seed_prices(Some(0.125));
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = setup(&store, AgentKind::Codex);
    store
        .db()
        .execute("UPDATE tasks SET model = 'component-test-model'", [])
        .expect("model");
    ingest(
        &store,
        &id,
        get_agent(AgentKind::Codex).as_ref(),
        crate::cost::test_support::CODEX_USAGE,
    );
    store.fail_active_execution(id.as_str()).expect("failure");
    assert_usage(&store, &id, 238_440, 0.111981);
}

#[test]
fn normal_completion_values_take_precedence_over_checkpoint() {
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = setup(&store, AgentKind::Claude);
    ingest(&store, &id, &crate::agent::claude::ClaudeAgent, RESULTS);
    complete(&store, &id, TaskStatus::Done, Some(12_000_000), Some(5.0));
    assert_usage(&store, &id, 12_000_000, 5.0);
    store
        .update_task_status(id.as_str(), TaskStatus::Merged)
        .expect("merge status");
    assert_usage(&store, &id, 12_000_000, 5.0);
}

#[test]
fn result_with_no_change_text_still_checkpoints_usage() {
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = setup(&store, AgentKind::Claude);
    ingest(
        &store,
        &id,
        &crate::agent::claude::ClaudeAgent,
        concat!(
            r#"{"type":"result","result":"NO_CHANGES_NEEDED: done","#,
            r#""usage":{"input_tokens":5},"total_cost_usd":0.1}"#
        ),
    );
    store.fail_active_execution(id.as_str()).expect("failure");
    assert_usage(&store, &id, 5, 0.1);
}

#[test]
fn event_and_checkpoint_roll_back_together() {
    let store = Store::open_memory().expect("store");
    let id = setup(&store, AgentKind::Claude);
    store
        .db()
        .execute_batch(
            "CREATE TRIGGER reject_usage BEFORE INSERT ON task_usage_checkpoints
         BEGIN SELECT RAISE(ABORT, 'checkpoint rejected'); END;",
        )
        .expect("trigger");
    let event = crate::agent::claude::ClaudeAgent
        .parse_event(&id, RESULTS.lines().next().expect("line"))
        .expect("event");
    let mut info = info();
    info.tokens = Some(1);
    assert!(store.insert_agent_event_with_usage(&event, &info).is_err());
    assert!(store.get_events(id.as_str()).expect("events").is_empty());
}

#[test]
fn cost_only_checkpoint_keeps_token_count_unknown() {
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = setup(&store, AgentKind::Claude);
    ingest(
        &store,
        &id,
        &crate::agent::claude::ClaudeAgent,
        r#"{"type":"result","total_cost_usd":0.5}"#,
    );
    store.fail_active_execution(id.as_str()).expect("failure");
    let task = store.get_task(id.as_str()).expect("query").expect("task");
    assert_eq!(task.tokens, None);
    assert_eq!(task.cost_usd, Some(0.5));
}
