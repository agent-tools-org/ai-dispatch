// Overage totals follow persisted dispatch evidence, task periods, and agent filters.
// Covers deduplication, known estimates, unknown prices and text/JSON fields.

use super::*;
use crate::routing::quota::live::CacheDirGuard;

use crate::types::{AgentKind, EventKind, Task, TaskEvent, TaskId, TaskStatus};

fn task() -> Task {
    crate::types::Task::test_fixture(TaskId("t-billing".into()), TaskStatus::Running)
}

fn overage(store: &Store, task: &Task) {
    store.insert_task(task).expect("task");
    for _ in 0..2 {
        store
            .insert_event(&TaskEvent {
                task_id: task.id.clone(),
                timestamp: Local::now(),
                event_kind: EventKind::Setup,
                detail: "billing: overage".into(),
                metadata: Some(serde_json::json!({"billing_state":"overage"})),
            })
            .expect("event");
    }
}

#[test]
fn usage_counts_tasks_once_and_filters_period_and_agent() {
    let temp = crate::support::test_env::TestHome::new();
    let _cache = CacheDirGuard::set(temp.path());
    let store = Store::open_memory().expect("store");
    let now = Local::now();
    let mut priced = task();
    priced.id = TaskId("t-priced".into());
    priced.created_at = now - chrono::Duration::hours(1);
    priced.cost_usd = Some(1.25);
    overage(&store, &priced);
    let mut unknown = priced.clone();
    unknown.id = TaskId("t-unknown".into());
    unknown.cost_usd = None;
    overage(&store, &unknown);
    let mut old = priced.clone();
    old.id = TaskId("t-old".into());
    old.created_at = now - chrono::Duration::days(10);
    overage(&store, &old);
    let mut other = priced.clone();
    other.id = TaskId("t-other".into());
    other.agent = AgentKind::Cursor;
    overage(&store, &other);
    let mut included = priced.clone();
    included.id = TaskId("t-included".into());
    store.insert_task(&included).expect("included");
    let tasks = vec![priced, unknown, old, other, included];
    let rows = collect(&store, &tasks, Some("codex"), UsageWindow::Days(7), now).expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].tasks, 2);
    assert_eq!(rows[0].estimated_cost_usd, 1.25);
    assert_eq!(rows[0].unknown_cost_tasks, 1);
    let text = render(&rows);
    assert!(
        text.contains("codex overage: 2 tasks | estimated cost $1.25 + 1 unknown"),
        "{text}"
    );
    let json = serde_json::to_value(&rows).expect("json");
    assert_eq!(json[0]["tasks"], 2);
    assert_eq!(json[0]["estimated_cost_usd"], 1.25);
    assert_eq!(json[0]["unknown_cost_tasks"], 1);
    let all = collect(&store, &tasks, None, UsageWindow::All, now).expect("all");
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].tasks, 3);
}

#[test]
fn json_preserves_existing_usage_view_and_adds_overage_rows() {
    let temp = crate::support::test_env::TestHome::new();
    let _cache = CacheDirGuard::set(temp.path());
    let analytics = crate::cost::usage::agent_analytics(&[], "codex", UsageWindow::All, Local::now());
    let rows = vec![OverageUsage {
        agent: "codex".into(),
        tasks: 2,
        estimated_cost_usd: 1.25,
        unknown_cost_tasks: 1,
    }];
    let text = json(UsageReport::Agent { analytics }, &rows).expect("report");
    let value: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert_eq!(value["view"], "agent");
    assert_eq!(value["analytics"]["agent_name"], "codex");
    assert_eq!(value["overage"][0]["tasks"], 2);
    assert_eq!(value["overage"][0]["unknown_cost_tasks"], 1);
}

#[test]
fn usage_ignores_overage_metadata_on_non_dispatch_events() {
    let temp = crate::support::test_env::TestHome::new();
    let _cache = CacheDirGuard::set(temp.path());
    let store = Store::open_memory().expect("store");
    let task = task();
    store.insert_task(&task).expect("task");
    store.insert_event(&TaskEvent {
        task_id: task.id.clone(), timestamp: Local::now(), event_kind: EventKind::Completion,
        detail: "unrelated completion metadata".into(),
        metadata: Some(serde_json::json!({"billing_state":"overage"})),
    }).expect("event");
    let rows = collect(&store, &[task], None, UsageWindow::All, Local::now()).expect("usage");
    assert!(rows.is_empty(), "only dispatch setup events identify overage");
}

#[test]
fn provider_pricing_stays_consistent_in_cost_reports_and_overage() {
    let temp = crate::support::test_env::TestHome::new();
    let _cache = CacheDirGuard::set(temp.path());
    let store = Store::open_memory().unwrap();
    let row = crate::routing::model_catalog::AGENT_MODELS
        .iter()
        .find(|row| row.agent == AgentKind::Codex && row.input_per_m > 0.0)
        .unwrap();
    let expected = row.input_per_m * 0.7 + row.output_per_m * 0.3;
    for name in [None, Some("codex-provider:X")] {
        let mut task = task();
        task.id = TaskId(
            if name.is_some() {
                "t-provider"
            } else {
                "t-default"
            }
            .into(),
        );
        task.custom_agent_name = name.map(str::to_string);
        task.requested_model = Some(row.model.into());
        task.tokens = Some(1_000_000);
        overage(&store, &task);
        assert_eq!(
            task.route().provider.as_str(),
            name.unwrap_or("openai-chatgpt-plan")
        );
        assert!(cost::has_known_price(task.costing_model(), task.agent));
        assert_eq!(cost::task_cost(&store, &task).unwrap(), Some(expected));
        let (_, totals) =
            crate::cmd::cost::daily_summary_rows(&[task.clone()], UsageWindow::All, Local::now());
        assert_eq!(totals.2, Some(expected));
        let rows = collect(&store, &[task], None, UsageWindow::All, Local::now()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].estimated_cost_usd, expected);
        assert_eq!(rows[0].unknown_cost_tasks, 0);
    }
}
