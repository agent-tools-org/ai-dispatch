// UTC SQL selection, missing-price display, and expiring-credit pace regressions.
// Shares route/task fixtures; no provider calls or running spend estimates.
use super::*;

#[test]
fn sql_selects_only_completed_route_tasks_in_the_utc_month() {
    let store = Store::open_memory().unwrap();
    let now = Utc.with_ymd_and_hms(2026, 10, 15, 12, 0, 0).unwrap();
    let (start, end) = month_bounds(now).unwrap();
    let mut current = task("current", start, 37.2);
    current.created_at = (start - chrono::Duration::days(3)).with_timezone(&Local);
    store.insert_task(&current).unwrap();
    for (id, at, status, name) in [
        (
            "old",
            start - chrono::Duration::seconds(1),
            TaskStatus::Done,
            "claude-api-ming",
        ),
        ("next", end, TaskStatus::Done, "claude-api-ming"),
        ("running", now, TaskStatus::Running, "claude-api-ming"),
        ("other", now, TaskStatus::Done, "claude-api-meet"),
    ] {
        let mut t = task(id, at, 100.0);
        t.status = status;
        t.custom_agent_name = Some(name.into());
        store.insert_task(&t).unwrap();
    }
    let tasks = store
        .api_route_month_tasks("claude-api-ming", start, end)
        .unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id.as_str(), "current");
    assert_eq!(calculate(&route(false), &tasks, now).unwrap().spent, 37.2);
}

#[test]
fn unknown_completed_cost_is_visible_and_reported_cost_is_authoritative() {
    let home = tempfile::tempdir().unwrap();
    let _guard = crate::support::paths::AidHomeGuard::set(home.path());
    crate::cost::clear_feed_for_tests();
    let now = Utc::now();
    let mut unpriced = task("unpriced", now, 0.0);
    unpriced.cost_usd = None;
    unpriced.tokens = Some(100);
    unpriced.observed_model = Some("claude-opus-99-9".into());
    let budget = calculate(&route(false), &[task("reported", now, 37.2), unpriced], now).unwrap();
    assert_eq!(budget.spent, 37.2);
    assert_eq!(budget.unpriced, 1);
    assert_eq!(budget.detail(), "$37.20 / $200 this month (1 tasks unpriced)");
}

#[test]
fn expiration_bonus_is_strictly_within_seven_days_and_below_eighty_percent_pace() {
    let before = Utc.with_ymd_and_hms(2026, 10, 25, 0, 0, 0).unwrap();
    let after = before + chrono::Duration::seconds(1);
    assert_eq!(calculate(&route(false), &[], before).unwrap().pace, 0.0);
    assert_eq!(calculate(&route(false), &[], after).unwrap().pace, 1.0);
    let now = Utc.with_ymd_and_hms(2026, 10, 26, 0, 0, 0).unwrap();
    let threshold = 200.0 * 25.0 / 31.0 * 0.8;
    assert_eq!(
        calculate(&route(false), &[task("at", now, threshold)], now)
            .unwrap()
            .pace,
        0.0
    );
    assert_eq!(
        calculate(&route(false), &[task("below", now, threshold - 0.01)], now)
            .unwrap()
            .pace,
        1.0
    );
    assert_eq!(
        calculate(&route(false), &[task("ahead", now, 200.0)], now)
            .unwrap()
            .pace,
        -2.0
    );
}

#[test]
fn launch_checks_completed_budget_without_installing_a_hold_or_changing_task_outcome() {
    let home = tempfile::tempdir().unwrap();
    let _guard = crate::support::paths::AidHomeGuard::set(home.path());
    let store = Store::open_memory().unwrap();
    store
        .insert_task(&task("finished-over", Utc::now(), 250.0))
        .unwrap();
    let agent = crate::agent::api_route::ApiRouteAgent(route(false));
    let error = crate::agent::api_route::validate_launch(&agent, &store, false).unwrap_err();
    assert!(error.to_string().contains("EXHAUSTED"));
    assert_eq!(
        store.get_task("finished-over").unwrap().unwrap().status,
        TaskStatus::Done
    );
    assert!(
        !crate::support::paths::aid_dir()
            .join("rate-limit-claude-api-ming")
            .exists()
    );
    let bill = crate::agent::api_route::ApiRouteAgent(route(true));
    crate::agent::api_route::validate_launch(&bill, &store, false).unwrap();
}

#[test]
fn completed_unreported_real_claude_id_uses_feed_price_even_with_opus_request() {
    let home = tempfile::tempdir().unwrap();
    let _guard = crate::support::paths::AidHomeGuard::set(home.path());
    let feed: crate::cost::price_feed::Feed = serde_json::from_value(serde_json::json!({
        "built_at": Utc::now().to_rfc3339(), "stale":false, "age_seconds":0,
        "models":[{"id":"anthropic/claude-opus-5.5", "aliases":["opus"],
            "input_per_mtok":4.0,"output_per_mtok":20.0}]
    }))
    .unwrap();
    crate::cost::clear_feed_for_tests();
    crate::cost::set_feed_for_tests(feed);
    let store = Store::open_memory().unwrap();
    let mut completed = task("real-id", Utc::now(), 0.0);
    completed.cost_usd = None;
    completed.tokens = Some(1_000_000);
    completed.requested_model = Some("opus".into());
    completed.observed_model = Some("claude-opus-5-5".into());
    store.insert_task(&completed).unwrap();
    let budget = for_route(&route(false), Some(&store)).unwrap();
    assert_eq!(budget.spent, 8.8);
    assert_eq!(budget.unpriced, 0);
}
