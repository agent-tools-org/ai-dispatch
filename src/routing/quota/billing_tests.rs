// Billing contract parsing and declared-profile policy matrix.
// Tests isolate aidbar cache and AID_HOME for all live reads.

use super::*;
use crate::routing::quota::live::{self, CacheDirGuard};
use crate::support::paths::AidHomeGuard;
use crate::types::{AgentKind, TaskDifficulty, TaskUrgency};

fn profile(budget: TaskBudget, rigor: TaskRigor) -> DeclaredTaskProfile {
    DeclaredTaskProfile {
        difficulty: TaskDifficulty::Moderate,
        budget,
        rigor,
        urgency: TaskUrgency::Normal,
    }
}

#[test]
fn billing_policy_matrix() {
    for budget in [
        TaskBudget::Free,
        TaskBudget::Cheap,
        TaskBudget::Standard,
        TaskBudget::Premium,
    ] {
        for rigor in [TaskRigor::Standard, TaskRigor::Critical] {
            let declared = profile(budget, rigor);
            for state in [
                BillingState::InPlan,
                BillingState::Overage,
                BillingState::Exhausted,
                BillingState::Unknown,
            ] {
                let overage = state == BillingState::Overage;
                let exhausted = state == BillingState::Exhausted;
                let billing = Billing { state, credits: None };
                let excluded =
                    exhausted || (overage && matches!(budget, TaskBudget::Free | TaskBudget::Cheap));
                assert_eq!(billing.exclusion(declared).is_some(), excluded);
                let penalty = if overage && budget == TaskBudget::Standard && rigor != TaskRigor::Critical {
                    -4.0
                } else {
                    0.0
                };
                assert_eq!(billing.penalty(declared), penalty);
            }
        }
    }
}

#[test]
fn cache_billing_preserves_optional_contract_and_rejects_stale_or_failed_evidence() {
    let temp = tempfile::tempdir().expect("temp");
    let _home = AidHomeGuard::set(temp.path());
    let _cache = CacheDirGuard::set(temp.path());
    for (billing, ok, fetched, expected) in cache_cases() {
        let mut snapshot = serde_json::json!({"provider":"codex","windows":[],"fetched_at":fetched});
        if let Some(billing) = billing {
            snapshot["billing"] = billing;
        }
        std::fs::write(
            temp.path().join("codex.json"),
            serde_json::to_vec(&serde_json::json!({"ok":ok,"snapshot":snapshot})).expect("json"),
        )
        .expect("cache");
        assert!(live::snapshot(&AgentKind::Codex).is_some());
        assert_eq!(
            live::billing(&AgentKind::Codex)
                .map(|billing| billing.label())
                .as_deref(),
            expected
        );
    }
}

type CacheCase = (
    Option<serde_json::Value>,
    bool,
    &'static str,
    Option<&'static str>,
);

fn cache_cases() -> [CacheCase; 6] {
    [
        (None, true, "2099-01-01T00:00:00Z", None),
        (
            Some(serde_json::json!({"state":"in_plan"})),
            true,
            "2099-01-01T00:00:00Z",
            Some("in_plan"),
        ),
        (
            Some(serde_json::json!({"state":"overage","credits":{"balance":48698.135,"unlimited":false}})),
            true,
            "2099-01-01T00:00:00Z",
            Some("overage (credits balance 48698.135)"),
        ),
        (
            Some(serde_json::json!({"state":"exhausted"})),
            true,
            "2099-01-01T00:00:00Z",
            Some("exhausted"),
        ),
        (
            Some(serde_json::json!({"state":"overage"})),
            false,
            "2099-01-01T00:00:00Z",
            None,
        ),
        (
            Some(serde_json::json!({"state":"overage"})),
            true,
            "2000-01-01T00:00:00Z",
            None,
        ),
    ]
}

#[test]
fn malformed_billing_preserves_probe_and_marker_release() {
    let temp = tempfile::tempdir().expect("temp");
    let _home = AidHomeGuard::set(temp.path());
    let _cache = CacheDirGuard::set(temp.path());
    let marker = temp.path().join("rate-limit-codex");
    std::fs::write(&marker, "recovery_at: Jan 01, 2099 2:23 PM\nmessage: usage limit\n")
        .expect("marker");
    for billing in [
        serde_json::json!({"state":"overage","credits":{"balance":null,"unlimited":false}}),
        serde_json::json!({"credits":{"balance":1,"unlimited":false}}),
        serde_json::json!({"state":"overage","credits":{"balance":"bad","unlimited":false}}),
        serde_json::json!({"state":"overage","credits":{"balance":1}}),
        serde_json::json!({"state":"future_state"}),
        serde_json::json!({"state":"overage","credits":{"balance":"overflow-number","unlimited":false}}),
        serde_json::json!(["overage"]),
    ] {
        let raw = serde_json::json!({"ok":true,"snapshot":{"provider":"codex",
            "windows":[{"label":"Weekly","used_percent":12.5}],
            "fetched_at":"2099-01-01T00:00:00Z","billing":billing}});
        let raw = raw.to_string().replace(r#""overflow-number""#, "1e999");
        std::fs::write(temp.path().join("codex.json"), raw).expect("cache");
        let probe = live::snapshot(&AgentKind::Codex).expect("probe survives malformed billing");
        assert_eq!(probe.windows[0].used_percent, 12.5);
        assert!(live::billing(&AgentKind::Codex).is_none());
        assert!({
            let _cache = crate::routing::quota::live::CacheDirGuard::set(temp.path());
            crate::routing::availability::from_path(&AgentKind::Codex, None, None, &marker).status
                == crate::routing::availability::RouteStatus::Dispatchable
        });
    }
}
