// API advice displays route billing and independent pace/hold penalties.
// Tests the full advice engine against persisted account costs.
use super::*;
use crate::support::paths::{self, AidHomeGuard};

#[test]
fn named_api_candidates_have_independent_billing_pace_and_holds() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    std::fs::write(
        paths::config_path(),
        concat!(
            "[[api_route]]\nid = 'claude-api-ming'\nagent = 'claude'\n",
            "keychain_service = 'test'\nmonthly_budget_usd = 200\n",
            "[[api_route]]\nid = 'claude-api-meet'\nagent = 'claude'\n",
            "keychain_service = 'test'\nmonthly_budget_usd = 200\nover_budget = 'bill'\n",
        ),
    )
    .unwrap();
    let store = Store::open_memory().unwrap();
    store
        .insert_waiting_task(
            "t-spend",
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
        cost_usd = 200 WHERE id = 't-spend'",
            [],
        )
        .unwrap();
    crate::routing::api_budget::hold("claude-api-meet", "test human hold", false).unwrap();
    let report = advise(
        "test",
        DeclaredTaskProfile {
            difficulty: Default::default(),
            budget: Default::default(),
            urgency: Default::default(),
            rigor: Default::default(),
        },
        None,
        None,
        Some(&store),
        0,
        None,
    );
    let ming = report
        .candidates
        .iter()
        .find(|c| c.agent == "claude-api-ming")
        .unwrap();
    let meet = report
        .candidates
        .iter()
        .find(|c| c.agent == "claude-api-meet")
        .unwrap();
    assert_eq!(
        ming.billing.as_ref().unwrap().state,
        crate::routing::quota::billing::BillingState::Exhausted
    );
    assert!(ming.exclusion_codes.iter().any(|code| code == "billing"));
    assert!(ming.breakdown.pace == -2.0 || ming.breakdown.pace == 0.0);
    assert_eq!(
        meet.billing.as_ref().unwrap().state,
        crate::routing::quota::billing::BillingState::InPlan
    );
    assert_eq!(meet.quota.status, "held");
    assert_eq!(ming.quota.status, "unknown");
    assert_eq!(meet.breakdown.pace, 0.0);
    assert!(
        !report
            .custom_candidates
            .iter()
            .any(|c| c.agent.starts_with("claude-api-"))
    );
}

#[test]
fn history_scores_do_not_share_other_accounts_or_claude() {
    let store = Store::open_memory().unwrap();
    for index in 0..5 {
        let id = format!("t-api-history-{index}");
        store
            .insert_waiting_task(
                &id, "claude", "test", None, None, None, None, None, None, false, false,
            )
            .unwrap();
        store
            .db()
            .execute(
                "UPDATE tasks SET agent = 'custom', custom_agent_name = 'claude-api-ming',
            status = 'done' WHERE id = ?1",
                [&id],
            )
            .unwrap();
    }
    assert_eq!(history_bonus(Some(&store), "claude-api-ming"), 4.0);
    assert_eq!(history_bonus(Some(&store), "claude-api-meet"), 0.0);
    assert_eq!(history_bonus(Some(&store), "claude"), 0.0);
}
