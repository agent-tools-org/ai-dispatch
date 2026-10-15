// Completion-month billing, live estimates, and stop-route launch caps.
// Shares synthetic route/task fixtures with monthly accounting tests.
use super::*;
use crate::agent::api_route::{ApiRouteAgent, launch_cost_cap};

#[test]
fn completion_month_and_running_now_determine_attribution() {
    let now = Local.with_ymd_and_hms(2026, 10, 15, 12, 0, 0).unwrap();
    let (start, _) = month_bounds(now).unwrap();
    let mut completed_now = task("cross-month", start - chrono::Duration::days(2), 10.0);
    completed_now.completed_at = Some(now);
    let mut completed_before = task("old-completion", now, 30.0);
    completed_before.completed_at = Some(start - chrono::Duration::seconds(1));
    let mut running = task("running", start - chrono::Duration::days(2), 5.0);
    running.status = TaskStatus::Running;
    running.completed_at = None;
    let budget = calculate(&route(false), &[completed_now, completed_before, running], now).unwrap();
    assert_eq!(budget.spent, 15.0);
}

#[test]
fn launch_cap_includes_running_estimates_and_respects_explicit_lower_cap() {
    let home = tempfile::tempdir().unwrap();
    let _guard = crate::support::paths::AidHomeGuard::set(home.path());
    let store = Store::open_memory().unwrap();
    let now = Local::now();
    store.insert_task(&task("reported", now, 37.2)).unwrap();
    let mut running = task("running", now, 0.0);
    running.status = TaskStatus::Running;
    running.completed_at = None;
    running.cost_usd = None;
    running.tokens = Some(1_000_000);
    running.requested_model = Some("sonnet".into());
    store.insert_task(&running).unwrap();
    let estimate = crate::cost::estimate_cost(1_000_000, Some("sonnet"), AgentKind::Claude).unwrap();
    let agent = ApiRouteAgent(route(false));
    let budget = for_route(&agent.0, Some(&store)).unwrap();
    assert_eq!(budget.spent, 37.2 + estimate);
    assert_eq!(
        launch_cost_cap(&agent, &store, None).unwrap(),
        Some(200.0 - budget.spent)
    );
    assert_eq!(launch_cost_cap(&agent, &store, Some(2.0)).unwrap(), Some(2.0));
    running.id = TaskId("oversubscribed".into());
    running.tokens = Some(100_000_000);
    store.insert_task(&running).unwrap();
    assert_eq!(
        for_route(&agent.0, Some(&store)).unwrap().billing.state,
        BillingState::Exhausted
    );
    assert!(launch_cost_cap(&agent, &store, None).is_err());
    assert_eq!(
        launch_cost_cap(&ApiRouteAgent(route(true)), &store, None).unwrap(),
        None
    );
}

#[test]
fn unreported_stopped_task_counts_estimated_spend() {
    let home = tempfile::tempdir().unwrap();
    let _guard = crate::support::paths::AidHomeGuard::set(home.path());
    let store = Store::open_memory().unwrap();
    let mut stopped = task("stopped-without-result", Local::now(), 0.0);
    stopped.status = TaskStatus::Stopped;
    stopped.cost_usd = None;
    stopped.tokens = Some(1_000_000);
    stopped.requested_model = Some("sonnet".into());
    store.insert_task(&stopped).unwrap();
    let expected = crate::cost::estimate_cost(1_000_000, Some("sonnet"), AgentKind::Claude).unwrap();
    assert_eq!(for_route(&route(false), Some(&store)).unwrap().spent, expected);
}
