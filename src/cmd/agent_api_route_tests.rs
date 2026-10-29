// API account quota display is distinct from the Claude subscription.
// Uses an empty temporary database, without reading any real keychain item.
use super::*;
use crate::support::paths::{self};

#[test]
fn api_quota_rows_show_plan_state_and_monthly_dollars() {
    let _home = crate::support::test_env::TestHome::new();
    std::fs::write(
        paths::config_path(),
        concat!(
            "[[api_route]]\nid = 'claude-api-meet'\nagent = 'claude'\n",
            "keychain_service = 'test'\nmonthly_budget_usd = 200\n",
        ),
    )
    .unwrap();
    let line = api_quota_line("claude-api-meet").unwrap().unwrap();
    assert!(line.contains("claude-api-meet"));
    assert!(line.contains("IN PLAN"));
    assert!(line.contains("$0.00 / $200 this month"));
    assert!(api_quota_line("claude").unwrap().is_none());
    show_quota().unwrap();
    list_agents().unwrap();
}

#[test]
fn api_quota_displays_exhausted_and_bill_overage() {
    let _home = crate::support::test_env::TestHome::new();
    let config = concat!(
        "[[api_route]]\nid = 'claude-api-meet'\nagent = 'claude'\n",
        "keychain_service = 'test'\nmonthly_budget_usd = 200\nover_budget = 'stop'\n",
    );
    std::fs::write(paths::config_path(), config).unwrap();
    let store = crate::store::Store::open(&paths::db_path()).unwrap();
    store
        .insert_waiting_task(
            "t-spend", "claude", "test", None, None, None, None, None, None, false, false,
        )
        .unwrap();
    store
        .db()
        .execute(
            "UPDATE tasks SET agent = 'custom', custom_agent_name = 'claude-api-meet',
        cost_usd = 200, status = 'done', completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = 't-spend'",
            [],
        )
        .unwrap();
    let line = api_quota_line("claude-api-meet").unwrap().unwrap();
    assert!(line.contains("EXHAUSTED"));
    assert!(line.contains("$200.00 / $200 this month"));
    std::fs::write(paths::config_path(), config.replace("'stop'", "'bill'")).unwrap();
    assert!(
        api_quota_line("claude-api-meet")
            .unwrap()
            .unwrap()
            .contains("OVERAGE")
    );
}

#[test]
fn quota_displays_unknown_completed_task_prices() {
    let _home = crate::support::test_env::TestHome::new();
    std::fs::write(
        paths::config_path(),
        concat!(
            "[[api_route]]\nid = 'claude-api-ming'\nagent = 'claude'\n",
            "keychain_service = 'test'\nmonthly_budget_usd = 200\n"
        ),
    )
    .unwrap();
    let store = crate::store::Store::open(&paths::db_path()).unwrap();
    store
        .insert_waiting_task(
            "t-unpriced",
            "claude-api-ming",
            "test",
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
    store
        .db()
        .execute(
            "UPDATE tasks SET agent = 'custom', custom_agent_name = 'claude-api-ming',
        status = 'failed', completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
        observed_model = 'claude-opus-99-9', tokens = 100 WHERE id = 't-unpriced'",
            [],
        )
        .unwrap();
    let line = api_quota_line("claude-api-ming").unwrap().unwrap();
    assert!(line.contains("$0.00 / $200 this month (1 tasks unpriced)"));
}
