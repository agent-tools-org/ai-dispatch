// API account quota display is distinct from the Claude subscription.
// Uses an empty temporary database, without reading any real keychain item.
use super::*;
use crate::support::paths::{self, AidHomeGuard};

#[test]
fn api_quota_rows_show_plan_state_and_monthly_dollars() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
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
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
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
        cost_usd = 200 WHERE id = 't-spend'",
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
