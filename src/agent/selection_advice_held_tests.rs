// Held-route recommendation regression coverage.
// Uses isolated advice fixtures and route-wide refusal markers.

use super::tests::{declared, find, isolated};
use super::*;
use crate::types::{TaskDifficulty, TaskRigor};

#[test]
fn needs_human_route_is_never_recommended_even_for_background() {
    let (_temp, _home, _cache) = isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Grok]);
    let marker = crate::routing::rate_limit::marker_path(&AgentKind::Grok, None);
    std::fs::create_dir_all(marker.parent().expect("parent")).expect("aid dir");
    std::fs::write(
        marker,
        "hold: manual\nmessage: API error (status 402 Payment Required)\n",
    )
    .expect("marker");
    for urgency in [TaskUrgency::Normal, TaskUrgency::Background] {
        let mut profile = declared(TaskDifficulty::Moderate, TaskBudget::Standard);
        profile.urgency = urgency;
        let report = advise("refactor scheduler", profile, None, None, None, 0, None);
        let grok = find(&report, "grok");
        assert_eq!(grok.quota.status, "held");
        assert!(!grok.eligible);
        assert!(grok.exclusion_codes.iter().any(|code| code == "held"));
        assert!(report.recommended.is_none());
    }
}

#[test]
fn billing_advice_excludes_or_penalizes_and_preserves_unknown_behavior() {
    let (temp, _home, _cache) = isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Codex]);
    let baseline = billing_report(declared(TaskDifficulty::Moderate, TaskBudget::Standard));
    let before = find(&baseline, "codex");
    for (state, budget, rigor, excluded, penalty) in [
        ("overage", TaskBudget::Free, TaskRigor::Standard, true, 0.0),
        ("overage", TaskBudget::Cheap, TaskRigor::Standard, true, 0.0),
        ("overage", TaskBudget::Standard, TaskRigor::Standard, false, -4.0),
        ("overage", TaskBudget::Premium, TaskRigor::Standard, false, 0.0),
        ("overage", TaskBudget::Standard, TaskRigor::Critical, false, 0.0),
        ("exhausted", TaskBudget::Premium, TaskRigor::Critical, true, 0.0),
        ("in_plan", TaskBudget::Standard, TaskRigor::Standard, false, 0.0),
    ] {
        write_billing(&temp, state);
        let mut profile = declared(TaskDifficulty::Moderate, budget);
        profile.rigor = rigor;
        let report = billing_report(profile);
        let codex = find(&report, "codex");
        assert_eq!(
            codex.exclusion_codes.iter().any(|code| code == "billing"),
            excluded
        );
        assert_eq!(codex.breakdown.billing, penalty);
        let credits = codex
            .billing
            .as_ref()
            .expect("billing")
            .credits
            .as_ref()
            .expect("credits");
        assert_eq!(credits.balance, 42.5);
        if excluded {
            assert!(!codex.launchable(None));
            assert!(report.recommended.is_none());
        }
        if budget == TaskBudget::Standard && rigor == TaskRigor::Standard {
            assert_eq!(codex.score, before.score + penalty);
        }
    }
    std::fs::remove_file(temp.path().join("aidbar/codex.json")).expect("remove cache");
    let after = billing_report(declared(TaskDifficulty::Moderate, TaskBudget::Standard));
    assert_eq!(find(&after, "codex").score, before.score);
    assert!(find(&after, "codex").billing.is_none());
}

fn write_billing(temp: &tempfile::TempDir, state: &str) {
    std::fs::write(
        temp.path().join("aidbar/codex.json"),
        serde_json::json!({
            "ok":true,"snapshot":{"provider":"codex","windows":[],
                "fetched_at":"2099-01-01T00:00:00Z", "billing":{"state":state,
                    "credits":{"balance":42.5,"unlimited":false}}}
        })
        .to_string(),
    )
    .expect("cache");
}

fn billing_report(profile: DeclaredTaskProfile) -> AdviceReport {
    advise(
        "refactor the scheduler",
        profile,
        Some(TaskCategory::Refactoring),
        None,
        None,
        0,
        None,
    )
}
