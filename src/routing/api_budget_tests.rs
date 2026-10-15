// Monthly accounting, local reset boundaries, pace and route independence.
// Synthetic task costs exercise the production calculator without provider calls.
use super::*;
use crate::types::{AgentKind, TaskId, TaskStatus, VerifyStatus};

fn route(bill: bool) -> ApiRoute {
    toml::from_str(&format!(
        "id = 'claude-api-ming'\nagent = 'claude'\n\
        keychain_service = 'test'\nmonthly_budget_usd = 200\nover_budget = '{}'",
        if bill { "bill" } else { "stop" }
    ))
    .unwrap()
}

fn task(id: &str, at: DateTime<Local>, cost: f64) -> Task {
    Task {
        id: TaskId(id.into()),
        agent: AgentKind::Custom,
        prompt: String::new(),
        resolved_prompt: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None,
        cost_usd: Some(cost),
        created_at: at,
        completed_at: Some(at),
        verify: None,
        read_only: false,
        budget: false,
        custom_agent_name: Some("claude-api-ming".into()),
        verify_status: VerifyStatus::Skipped,
        exit_code: Some(0),
        category: None,
        pending_reason: None,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
        final_head_sha: None,
        final_branch: None,
        observed_model: None,
        attribution_source: None,
        project_id: None,
        effective_dir: None,
    }
}

#[test]
fn monthly_spend_excludes_other_routes_subscription_previous_and_future_months() {
    let now = Local.with_ymd_and_hms(2026, 10, 15, 12, 0, 0).unwrap();
    let (start, end) = month_bounds(now).unwrap();
    let mut other = task("other", now, 100.0);
    other.custom_agent_name = Some("claude-api-meet".into());
    let mut plain = task("plain", now, 100.0);
    plain.agent = AgentKind::Claude;
    plain.custom_agent_name = None;
    let tasks = vec![
        task("old", start - chrono::Duration::seconds(1), 100.0),
        other,
        plain,
        task("current", start, 37.2),
        task("future", end, 100.0),
    ];
    let budget = calculate(&route(false), &tasks, now).unwrap();
    assert_eq!(budget.spent, 37.2);
    assert_eq!(budget.billing.state, BillingState::InPlan);
    assert_eq!(budget.detail(), "$37.20 / $200 this month");
    assert_eq!(budget.pace, 0.0);
}

#[test]
fn budget_threshold_and_pace_are_separate() {
    let now = Local.with_ymd_and_hms(2026, 10, 2, 0, 0, 0).unwrap();
    for (cost, bill, state, pace) in [
        (0.0, false, BillingState::InPlan, 0.0),
        (199.99, false, BillingState::InPlan, -2.0),
        (200.0, false, BillingState::Exhausted, -2.0),
        (201.0, false, BillingState::Exhausted, -2.0),
        (200.0, true, BillingState::Overage, -2.0),
        (201.0, true, BillingState::Overage, -2.0),
    ] {
        let result = calculate(&route(bill), &[task("cost", now, cost)], now).unwrap();
        assert_eq!(result.billing.state, state);
        assert_eq!(result.pace, pace);
    }
}

#[test]
fn december_rolls_to_next_year_at_local_midnight() {
    let now = Local.with_ymd_and_hms(2026, 12, 31, 23, 59, 59).unwrap();
    let (_, end) = month_bounds(now).unwrap();
    assert_eq!(end, Local.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap());
}

#[test]
fn read_only_accounting_uses_persisted_route_costs() {
    let store = Store::open_memory().unwrap();
    let now = Local::now();
    store.insert_task(&task("persisted", now, 37.2)).unwrap();
    let budget = for_route(&route(false), Some(&store)).unwrap();
    assert_eq!(budget.spent, 37.2);
}

#[test]
fn pace_penalty_starts_strictly_above_the_elapsed_threshold() {
    let now = Local.with_ymd_and_hms(2026, 10, 16, 12, 0, 0).unwrap();
    let at_threshold = calculate(&route(false), &[task("pace", now, 125.0)], now).unwrap();
    let above_threshold = calculate(&route(false), &[task("pace", now, 125.01)], now).unwrap();
    assert_eq!(at_threshold.pace, 0.0);
    assert_eq!(above_threshold.pace, -2.0);
    assert_eq!(above_threshold.billing.state, BillingState::InPlan);
}

#[test]
fn empty_spend_is_positive_zero_in_quota_display() {
    let budget = calculate(&route(false), &[], Local::now()).unwrap();
    assert!(budget.spent.is_sign_positive());
    assert_eq!(budget.detail(), "$0.00 / $200 this month");
}

#[path = "api_budget_admission_tests.rs"]
mod admission_tests;
