// Candidate text regression coverage for unknown and known billing evidence.
// Uses isolated route inventory and explicitly pinned score terms.

use super::*;
use crate::routing::quota::billing::{Billing, BillingState};
use crate::types::{AgentKind, DeclaredTaskProfile};

#[test]
fn unknown_billing_keeps_original_candidate_line_and_known_billing_adds_terms() {
    let temp = tempfile::tempdir().expect("temp");
    let _home = crate::support::paths::AidHomeGuard::set(temp.path());
    let _cache = crate::routing::quota::live::CacheDirGuard::set(temp.path());
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Grok]);
    let report = crate::agent::selection::advise(
        "refactor scheduler",
        DeclaredTaskProfile {
            difficulty: crate::types::TaskDifficulty::Moderate,
            budget: crate::types::TaskBudget::Standard,
            urgency: crate::types::TaskUrgency::Normal,
            rigor: crate::types::TaskRigor::Standard,
        },
        None,
        None,
        None,
        0,
        None,
    );
    let mut candidate = baseline_candidate(report);
    let model = crate::agent::run_model::model_label(None, false, candidate.source);
    let expected = format!(
        concat!(
            "  1. grok         6.0  base 6.0  +0.0 model  +0.0 budget  +0.0 limit",
            "  +0.0 history  +0.0 team  +0.0 headroom  model {}"
        ),
        model
    );
    assert_eq!(candidate_line(0, &candidate), expected);
    candidate.billing = Some(Billing {
        state: BillingState::Overage,
        credits: None,
    });
    candidate.breakdown.billing = -4.0;
    let known = candidate_line(0, &candidate);
    assert!(known.contains("  -4.0 billing  model "), "{known}");
    assert!(known.ends_with("  overage"), "{known}");
}

fn baseline_candidate(report: AdviceReport) -> crate::agent::selection::AdviceCandidate {
    let mut candidate = report
        .candidates
        .into_iter()
        .find(|item| item.agent == "grok")
        .expect("grok");
    candidate.score = 6.0;
    candidate.breakdown.base = 6.0;
    candidate.breakdown.model_capability = 0.0;
    candidate.breakdown.budget_penalty = 0.0;
    candidate.breakdown.rate_limit_penalty = 0.0;
    candidate.breakdown.history_bonus = 0.0;
    candidate.breakdown.team_bonus = 0.0;
    candidate.breakdown.headroom_penalty = 0.0;
    candidate.exclusion_reason = None;
    candidate.demotion_reason = None;
    candidate.model = None;
    candidate.pinned = false;
    candidate
}
