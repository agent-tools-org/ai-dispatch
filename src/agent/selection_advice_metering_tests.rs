// Cross-candidate prepaid preference regression tests.
// Exercises ranking terms and the notice consumed by explicit dispatch.
use super::*;

fn candidates(status: &str) -> Vec<RankedCandidate> {
    let report = advise(
        "refactor",
        DeclaredTaskProfile {
            difficulty: Default::default(),
            budget: Default::default(),
            urgency: Default::default(),
            rigor: Default::default(),
        },
        None,
        None,
        None,
        0,
        None,
    );
    [("cursor", 6.0), ("opencode", 9.0)]
        .into_iter()
        .map(|(name, score)| {
            let mut report = report
                .candidates
                .iter()
                .find(|c| c.agent == name)
                .expect("route")
                .clone();
            report.installed = true;
            report.eligible = true;
            report.score = score;
            report.breakdown.total = score;
            report.quota.status = if name == "cursor" { status } else { "unknown" }.into();
            report.quota.used_percent = (name == "cursor").then_some(8.6);
            RankedCandidate {
                report,
                order: Candidate {
                    kind: AgentKind::Custom,
                    score,
                    efficiency: score,
                    is_default: false,
                    priority: 0,
                },
                pool_excluded: false,
                capability_note: None,
            }
        })
        .collect()
}

#[test]
fn metered_leader_falls_below_usable_prepaid() {
    let (_temp, _home, _cache) = super::tests::isolated();
    let mut ranked = candidates("dispatchable");
    assert!(ranking_score(&ranked[1]) > ranking_score(&ranked[0]));
    metering::apply(&mut ranked);
    assert!(ranking_score(&ranked[1]) < ranking_score(&ranked[0]));
    let json = serde_json::to_value(&ranked[1].report).expect("json");
    assert_eq!(json["breakdown"]["metering"], -20.0);
}

#[test]
fn metered_fallback_keeps_rank_without_usable_prepaid() {
    let (_temp, _home, _cache) = super::tests::isolated();
    for status in ["degraded", "held", "limited"] {
        let mut ranked = candidates(status);
        metering::apply(&mut ranked);
        assert_eq!(ranked[1].report.breakdown.metering, 0.0);
        assert!(ranking_score(&ranked[1]) > ranking_score(&ranked[0]));
    }
}

#[test]
fn explicit_metered_notice_names_prepaid_usage() {
    let (_temp, _home, _cache) = super::tests::isolated();
    let mut ranked = candidates("dispatchable");
    metering::apply(&mut ranked);
    let mut report = advise(
        "refactor",
        DeclaredTaskProfile {
            difficulty: Default::default(),
            budget: Default::default(),
            urgency: Default::default(),
            rigor: Default::default(),
        },
        None,
        None,
        None,
        0,
        None,
    );
    report.candidates = ranked.into_iter().map(|c| c.report).collect();
    let notice = report.metered_notice("opencode").expect("notice");
    assert!(notice.contains("cursor (8.6% used)"));
    assert!(report.metered_notice("cursor").is_none());
}

#[test]
fn actual_advice_ranks_prepaid_before_stronger_metered_route() {
    let (_temp, _home, _cache) = super::tests::isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Cursor, AgentKind::OpenCode]);
    let team = crate::config::team::parse_team(
        "[team]\nid='test'\ndisplay_name='Test'\npreferred_agents=[]\n\
         [team.overrides.cursor]\nrefactoring=6\n[team.overrides.opencode]\nrefactoring=9\n",
    )
    .expect("team");
    let declared =
        super::tests::declared(crate::types::TaskDifficulty::Moderate, TaskBudget::Standard);
    let report = advise(
        "refactor scheduler",
        declared,
        Some(TaskCategory::Refactoring),
        Some(&team),
        None,
        0,
        None,
    );
    let cursor = report
        .candidates
        .iter()
        .position(|c| c.agent == "cursor")
        .expect("cursor");
    let metered = report
        .candidates
        .iter()
        .position(|c| c.agent == "opencode")
        .expect("opencode");
    assert!(cursor < metered);
    assert!(
        report.candidates[metered]
            .metering_reason
            .as_deref()
            .expect("reason")
            .contains("cursor")
    );
}

#[test]
fn excluded_prepaid_does_not_trigger_and_unknown_native_claude_does() {
    let (_temp, _home, _cache) = super::tests::isolated();
    let mut ranked = candidates("unknown");
    ranked[0].report.eligible = false;
    metering::apply(&mut ranked);
    assert_eq!(ranked[1].report.breakdown.metering, 0.0);
    ranked[0].report.eligible = true;
    ranked[0].report.agent = "claude".into();
    metering::apply(&mut ranked);
    assert_eq!(ranked[1].report.breakdown.metering, -20.0);
    assert!(
        ranked[1]
            .report
            .metering_reason
            .as_deref()
            .expect("reason")
            .contains("claude")
    );
}

#[test]
fn agy_subscription_family_pool_counts_as_prepaid() {
    let (_temp, _home, _cache) = super::tests::isolated();
    let mut ranked = candidates("dispatchable");
    ranked[0].report.agent = "agy".into();
    metering::apply(&mut ranked);
    assert_eq!(ranked[1].report.breakdown.metering, -20.0);
    assert!(
        ranked[1]
            .report
            .metering_reason
            .as_deref()
            .expect("reason")
            .contains("agy")
    );
}
